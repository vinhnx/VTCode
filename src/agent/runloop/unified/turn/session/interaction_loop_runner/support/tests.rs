use super::*;
use tempfile::TempDir;
use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use vtcode_config::{SubagentSource, SubagentSpec};
use vtcode_core::tools::tool_intent::GENERIC_VERIFIER_DESCRIPTION;

#[test]
fn program_status_live_reload_projects_opt_in_and_disable() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = vtcode_ui::tui::app::InlineHandle::new_for_tests(sender);
    let mut cfg = VTCodeConfig::default();
    let bootstrap = SessionBootstrap::default();
    for enabled in [true, false] {
        cfg.ui.program_status.enabled = enabled;
        apply_live_theme_and_appearance(&handle, &cfg, &bootstrap);
        let mut projections = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            if let vtcode_ui::tui::app::InlineCommand::ProgramStatus(update) = command {
                projections.push(update);
            }
        }
        assert_eq!(projections, vec![vtcode_commons::program_status::ProgramStatusUpdate::Configure { enabled }]);
    }
}

#[test]
fn next_primary_agent_name_starts_with_first_sorted_agent() {
    let specs = vec![test_subagent_spec("beta"), test_subagent_spec("alpha")];

    assert_eq!(next_primary_agent_name(&default_active_primary_agent(), &specs), Some("alpha".to_string()));
}

#[test]
fn next_primary_agent_name_cycles_to_next_sorted_agent() {
    let specs = vec![test_subagent_spec("beta"), test_subagent_spec("alpha")];
    let active = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[1]);

    assert_eq!(next_primary_agent_name(&active, &specs), Some("beta".to_string()));
}

#[test]
fn next_primary_agent_name_cycles_last_agent_to_first() {
    let specs = vec![test_subagent_spec("build"), test_subagent_spec("duck")];
    let active = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[1]);

    assert_eq!(next_primary_agent_name(&active, &specs), Some("build".to_string()));
}

#[test]
fn primary_agent_names_put_builtin_auto_last_before_custom_agents() {
    let specs = vec![
        test_subagent_spec("zeta"),
        test_subagent_spec("auto"),
        test_subagent_spec("plan"),
        test_subagent_spec("build"),
        test_subagent_spec("duck"),
        test_subagent_spec("alpha"),
    ];

    assert_eq!(
        primary_agent_names(&specs),
        vec![
            "build".to_string(),
            "duck".to_string(),
            "plan".to_string(),
            "auto".to_string(),
            "alpha".to_string(),
            "zeta".to_string(),
        ]
    );
}

#[test]
fn previous_primary_agent_name_cycles_through_explicit_builtin_order() {
    let specs = vec![
        test_subagent_spec("auto"),
        test_subagent_spec("plan"),
        test_subagent_spec("build"),
        test_subagent_spec("duck"),
    ];
    let active = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[0]);

    assert_eq!(previous_primary_agent_name(&active, &specs), Some("plan".to_string()));
}

#[test]
fn next_primary_agent_name_skips_non_primary_subagents() {
    let mut worker = test_subagent_spec("worker");
    worker.mode = vtcode_config::AgentMode::Subagent;
    let specs = vec![worker, test_subagent_spec("duck")];

    assert_eq!(next_primary_agent_name(&default_active_primary_agent(), &specs), Some("duck".to_string()));
}

#[test]
fn auto_and_plan_remain_reachable_when_cycling_agents() {
    let mut auto = test_subagent_spec("auto");
    auto.permissions = AgentPermissionsConfig::new(PermissionDefault::Auto);
    let specs = vec![test_subagent_spec("build"), test_subagent_spec("plan"), auto];

    let build = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[0]);
    let plan = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[1]);

    assert_eq!(next_primary_agent_name(&build, &specs), Some("plan".to_string()));
    assert_eq!(next_primary_agent_name(&plan, &specs), Some("auto".to_string()));
    assert!(agent_needs_trust(&specs, "auto"));
}

fn default_active_primary_agent() -> vtcode_core::primary_agent::ActivePrimaryAgent {
    vtcode_core::primary_agent::ActivePrimaryAgentState::default().active().clone()
}

fn test_subagent_spec(name: &str) -> SubagentSpec {
    SubagentSpec {
        name: name.to_string(),
        description: String::new(),
        prompt: String::new(),
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
        mode: vtcode_config::AgentMode::Primary,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::ProjectVtcode,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: BTreeMap::new(),
    }
}

#[test]
fn extract_recent_follow_up_hint_reads_latest_recoverable_tool_payload() {
    let history = vec![
        uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "error": "older",
                "is_recoverable": true,
                "fallback_tool": "read_file",
                "fallback_tool_args": {"path":"a.rs","offset":1,"limit":20}
            })
            .to_string(),
        ),
        uni::Message::tool_response(
            "call_2".to_string(),
            serde_json::json!({
                "error": "newer",
                "is_recoverable": true,
                "fallback_tool": "task_tracker",
                "fallback_tool_args": {"action":"list"}
            })
            .to_string(),
        ),
    ];

    let hint = extract_recent_follow_up_hint(&history);
    assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
}

#[test]
fn extract_recent_follow_up_hint_skips_non_recoverable_payloads() {
    let history = vec![uni::Message::tool_response(
        "call_1".to_string(),
        serde_json::json!({
            "error": "blocked",
            "is_recoverable": false,
            "fallback_tool": "read_file",
            "fallback_tool_args": {"path":"a.rs"}
        })
        .to_string(),
    )];

    assert!(extract_recent_follow_up_hint(&history).is_none());
}

#[test]
fn supports_native_openai_file_inputs_requires_openai_provider_and_responses_support() {
    assert!(supports_native_openai_file_inputs("openai", true));
    assert!(!supports_native_openai_file_inputs("openai", false));
    assert!(!supports_native_openai_file_inputs("anthropic", true));
    assert!(!supports_native_openai_file_inputs("mycorp", true));
}

#[test]
fn extract_recent_follow_up_hint_prefers_latest_hint() {
    let history = vec![
        uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "next_continue_args": {
                    "session_id": "run-42"
                }
            })
            .to_string(),
        ),
        uni::Message::tool_response(
            "call_2".to_string(),
            serde_json::json!({
                "fallback_tool": "task_tracker",
                "fallback_tool_args": {"action":"list"},
                "is_recoverable": true
            })
            .to_string(),
        ),
    ];
    let hint = extract_recent_follow_up_hint(&history);
    assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
}

#[test]
fn extract_recent_follow_up_hint_skips_stale_tool_hints_after_assistant_reply() {
    let history = vec![
        uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "next_continue_args": {
                    "session_id": "run-123"
                }
            })
            .to_string(),
        ),
        uni::Message::assistant("All done.".to_string()),
    ];

    assert!(extract_recent_follow_up_hint(&history).is_none());
}

#[test]
fn extract_recent_follow_up_hint_keeps_hint_when_system_note_follows_tool() {
    let history = vec![
        uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "fallback_tool": "task_tracker",
                "fallback_tool_args": {"action":"list"},
                "is_recoverable": true
            })
            .to_string(),
        ),
        uni::Message::system("Tool blocked; try fallback.".to_string()),
    ];

    let hint = extract_recent_follow_up_hint(&history);
    assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
}

#[test]
fn extract_recent_follow_up_hint_ignores_next_action_when_fallback_exists() {
    let history = vec![uni::Message::tool_response(
        "call_1".to_string(),
        serde_json::json!({
            "error": "Tool preflight validation failed: x",
            "is_recoverable": true,
            "fallback_tool": "task_tracker",
            "fallback_tool_args": {"action":"list"},
            "next_action": "Retry with fallback_tool_args."
        })
        .to_string(),
    )];

    let hint = extract_recent_follow_up_hint(&history);
    assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
}

#[test]
fn extract_recent_follow_up_hint_does_not_promote_next_action_only_payload() {
    let history = vec![uni::Message::tool_response(
        "call_1".to_string(),
        serde_json::json!({
            "error": "boom",
            "is_recoverable": true,
            "next_action": "Try an alternative tool or narrower scope."
        })
        .to_string(),
    )];

    assert!(extract_recent_follow_up_hint(&history).is_none());
}

#[test]
fn extract_recent_follow_up_hint_reads_next_continue_args() {
    let history = vec![uni::Message::tool_response(
        "call_1".to_string(),
        serde_json::json!({
            "next_continue_args": {
                "session_id": "run-123"
            }
        })
        .to_string(),
    )];

    let (tool_name, args) = extract_recent_follow_up_hint(&history).expect("continuation hint");
    assert_eq!(tool_name, tool_names::WRITE_STDIN, "continuation hints must use the public write_stdin tool");
    assert_ne!(tool_name, tool_names::UNIFIED_EXEC);
    assert_eq!(
        args,
        serde_json::json!({
            "session_id": "run-123",
            "chars": ""
        })
    );
    assert!(args.get("action").is_none());
}

#[test]
fn extract_recent_follow_up_hint_reads_next_read_args() {
    let history = vec![uni::Message::tool_response(
        "call_1".to_string(),
        serde_json::json!({
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/out.txt",
                "offset": 41,
                "limit": 40
            }
        })
        .to_string(),
    )];

    let hint = extract_recent_follow_up_hint(&history);
    assert_eq!(
        hint,
        Some((
            tool_names::UNIFIED_FILE.to_string(),
            serde_json::json!({
                "action": "read",
                "path": ".vtcode/context/tool_outputs/out.txt",
                "offset": 41,
                "limit": 40
            })
        ))
    );
}

#[test]
fn fallback_args_preview_truncates_long_payloads() {
    let args = serde_json::json!({
        "action": "search",
        "query": "x".repeat(600)
    });
    let preview = fallback_args_preview(&args);
    assert!(preview.ends_with("..."));
    assert!(preview.len() <= 243);
}

#[test]
fn stalled_follow_up_recovery_prompt_mentions_fallback_without_replacing_user_input() {
    let prompt = stalled_follow_up_recovery_prompt("turn blocked", true);

    assert!(prompt.contains("Stall reason: turn blocked"));
    assert!(prompt.contains("Use the recovered fallback hint"));
}

#[test]
fn stalled_verification_resume_directive_names_verifier_and_keeps_completion_gated() {
    let directive = stalled_verification_resume_directive(Some("go test ./..."));

    assert!(directive.contains("Run `go test ./...` with `exec_command`"));
    assert!(directive.contains("resume the original request"));
    assert!(directive.contains("The turn cannot complete while the gate is pending"));
    assert!(directive.contains(VERIFIER_SHELL_FORM_NOTE));
}

#[test]
fn stalled_verification_resume_directive_names_generic_verifier_without_project_marker() {
    let directive = stalled_verification_resume_directive(None);

    assert!(directive.contains(&format!("Run {GENERIC_VERIFIER_DESCRIPTION} with `exec_command`")));
    assert!(!directive.contains("Run `cargo check --locked`"), "no single command is presumed: {directive}");
    assert!(directive.contains("resume the original request"));
}

#[test]
fn build_file_reference_metadata_maps_aliases_to_full_paths() {
    let temp_dir = TempDir::new().expect("temp dir");
    let file_path = temp_dir.path().join("src").join("main.rs");
    fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
    fs::write(&file_path, "fn main() {}\n").expect("write file");

    let metadata = build_file_reference_metadata("check @src/main.rs and continue", temp_dir.path()).expect("metadata");

    assert!(metadata.contains("@src/main.rs="));
    assert!(metadata.contains("src/main.rs"));
}

#[test]
fn append_file_reference_metadata_keeps_ui_alias_but_adds_full_path_context() {
    let temp_dir = TempDir::new().expect("temp dir");
    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# test\n").expect("write file");

    let mut augmented = uni::MessageContent::text("check @README.md".to_string());
    append_file_reference_metadata(&mut augmented, "check @README.md", temp_dir.path());

    match augmented {
        uni::MessageContent::Text(text) => {
            assert!(text.contains("check @README.md"));
            assert!(text.contains("[file_reference_metadata]"));
            assert!(text.contains("@README.md="));
            assert!(text.contains("README.md"));
        }
        uni::MessageContent::Parts(_) => panic!("expected text content"),
    }
}

#[test]
fn append_agent_reference_metadata_adds_selected_agent_hint() {
    let mut augmented = uni::MessageContent::text("use rust-engineer agent".to_string());
    append_agent_reference_metadata(&mut augmented, &[String::from("rust-engineer")]);

    match augmented {
        uni::MessageContent::Text(text) => {
            assert!(text.contains("use rust-engineer agent"));
            assert!(text.contains("[agent_reference_metadata]"));
            assert!(text.contains("selected=@agent-rust-engineer"));
        }
        uni::MessageContent::Parts(_) => panic!("expected text content"),
    }
}

#[test]
fn build_file_reference_metadata_ignores_non_file_aliases() {
    let temp_dir = TempDir::new().expect("temp dir");
    let metadata = build_file_reference_metadata("npm i @types/node", temp_dir.path());
    assert!(metadata.is_none());
}

#[test]
fn build_file_reference_metadata_ignores_absolute_paths() {
    let temp_dir = TempDir::new().expect("temp dir");
    let metadata = build_file_reference_metadata("check @/tmp/example.rs", temp_dir.path());
    assert!(metadata.is_none());
}

#[test]
fn append_submitted_attachments_leaves_text_only_content_unchanged() {
    let mut content = uni::MessageContent::text("plain prompt".to_string());

    append_submitted_attachments(&mut content, &[]);

    assert_eq!(content, uni::MessageContent::text("plain prompt".to_string()));
}

#[tokio::test]
async fn parsed_at_image_stays_before_pasted_image() {
    let temp_dir = TempDir::new().expect("temp dir");
    let image_path = temp_dir.path().join("sample.png");
    fs::write(&image_path, tiny_png_bytes()).expect("write image");
    let mut content = vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
        "look @sample.png",
        temp_dir.path(),
        vtcode_core::utils::at_pattern::AtPatternOptions::default(),
    )
    .await
    .expect("parse image");

    append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

    assert_image_payload_order(&content, &["iVBOR", "pasted-image"]);
}

#[tokio::test]
async fn data_image_stays_before_pasted_image() {
    let mut content = vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
        "look data:image/png;base64,parsedimage",
        Path::new("."),
        vtcode_core::utils::at_pattern::AtPatternOptions::default(),
    )
    .await
    .expect("parse data image");

    append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

    assert_image_payload_order(&content, &["parsedimage", "pasted-image"]);
}

#[test]
fn clipboard_images_are_appended_after_refined_text_and_parsed_parts() {
    let mut content = uni::MessageContent::parts(vec![
        uni::ContentPart::text("refined prompt".to_string()),
        uni::ContentPart::image("parsed-image".to_string(), "image/png".to_string()),
    ]);

    append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

    match content {
        uni::MessageContent::Parts(parts) => {
            assert!(matches!(
                parts.as_slice(),
                [
                    uni::ContentPart::Text { .. },
                    uni::ContentPart::Image { .. },
                    uni::ContentPart::Image { .. }
                ]
            ));
            assert_eq!(
                image_payloads(&uni::MessageContent::parts(parts)),
                vec!["parsed-image".to_string(), "pasted-image".to_string()]
            );
        }
        uni::MessageContent::Text(_) => panic!("expected parts"),
    }
}

#[test]
fn inline_image_placeholder_text_stays_before_pasted_image_part() {
    let mut content = uni::MessageContent::text("[Image #1] and here?".to_string());

    append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

    match content {
        uni::MessageContent::Parts(parts) => {
            assert!(matches!(parts.as_slice(), [uni::ContentPart::Text { .. }, uni::ContentPart::Image { .. }]));
            assert_eq!(parts[0].as_text(), Some("[Image #1] and here?"));
            assert_eq!(image_payloads(&uni::MessageContent::parts(parts)), vec!["pasted-image".to_string()]);
        }
        uni::MessageContent::Text(_) => panic!("expected parts"),
    }
}

#[test]
fn metadata_remains_trailing_after_all_image_parts() {
    let temp_dir = TempDir::new().expect("temp dir");
    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# test\n").expect("write file");
    let mut content = uni::MessageContent::parts(vec![
        uni::ContentPart::text("check @README.md".to_string()),
        uni::ContentPart::image("parsed-image".to_string(), "image/png".to_string()),
    ]);
    append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

    append_file_reference_metadata(&mut content, "check @README.md", temp_dir.path());
    append_agent_reference_metadata(&mut content, &[String::from("rust-engineer")]);

    match content {
        uni::MessageContent::Parts(parts) => {
            assert!(matches!(parts[1], uni::ContentPart::Image { .. }));
            assert!(matches!(parts[2], uni::ContentPart::Image { .. }));
            assert!(matches!(parts[3], uni::ContentPart::Text { .. }));
            assert!(matches!(parts[4], uni::ContentPart::Text { .. }));
            assert!(parts[3].as_text().unwrap_or("").contains("[file_reference_metadata]"));
            assert!(parts[4].as_text().unwrap_or("").contains("[agent_reference_metadata]"));
        }
        uni::MessageContent::Text(_) => panic!("expected parts"),
    }
}

#[test]
fn unsupported_model_rejects_submitted_image_attachments() {
    let temp_dir = TempDir::new().expect("temp dir");
    let input = SubmittedInput::new("please inspect this", vec![UiContentPart::image("pasted-image", "image/png")]);

    assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
    assert!(!submitted_images_are_unsupported(&input, true, temp_dir.path()));
    assert_eq!(input.text, "please inspect this");
    assert_eq!(input.attachments, vec![UiContentPart::image("pasted-image", "image/png")]);
}

#[test]
fn unsupported_model_rejects_text_data_image() {
    let temp_dir = TempDir::new().expect("temp dir");
    let input = SubmittedInput::from("look data:image/png;base64,parsedimage".to_string());

    assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
    assert!(!submitted_images_are_unsupported(&input, true, temp_dir.path()));
    assert_eq!(input.text, "look data:image/png;base64,parsedimage");
    assert!(input.attachments.is_empty());
}

#[test]
fn selected_model_image_support_uses_image_modality_metadata() {
    assert!(selected_model_supports_image_input("openai", "gpt-5.6-sol", false));
}

#[test]
fn selected_model_image_support_uses_alias_modality_metadata() {
    // `gpt-5.6` is the documented family alias routing to `gpt-5.6-sol`;
    // modality metadata must come from the aliased model.
    assert!(selected_model_supports_image_input("openai", "gpt-5.6", false));
}

#[test]
fn selected_model_image_support_accepts_chatgpt_provider_label() {
    assert!(selected_model_supports_image_input("OpenAI (ChatGPT)", "gpt-5.6-sol", false));
}

#[test]
fn selected_model_image_support_accepts_display_model_label() {
    // Picker-style labels may carry a context suffix; the stripped prefix
    // must still resolve to the model's modality metadata.
    assert!(selected_model_supports_image_input("OpenAI (ChatGPT)", "gpt-5.6-sol (128K)", false));
}

#[test]
fn selected_model_image_support_rejects_text_only_metadata() {
    assert!(!selected_model_supports_image_input("openai", "gpt-oss-20b", true));
}

#[test]
fn selected_model_image_support_falls_back_without_metadata() {
    assert!(selected_model_supports_image_input("openai", "custom-vision-model", true));
    assert!(!selected_model_supports_image_input("custom-provider", "gpt-5.6-sol", false));
}

#[test]
fn unsupported_model_rejects_text_at_image_path() {
    let temp_dir = TempDir::new().expect("temp dir");
    let image_path = temp_dir.path().join("sample.png");
    fs::write(&image_path, tiny_png_bytes()).expect("write image");
    let input = SubmittedInput::from("look @sample.png".to_string());

    assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
    assert!(input.attachments.is_empty());
}

#[test]
fn unsupported_model_rejects_text_raw_image_path() {
    let temp_dir = TempDir::new().expect("temp dir");
    let image_path = temp_dir.path().join("raw.png");
    fs::write(&image_path, tiny_png_bytes()).expect("write image");
    let input = SubmittedInput::from(format!("look {}", image_path.display()));

    assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
    assert!(input.attachments.is_empty());
}

#[test]
fn unsupported_model_rejects_text_image_url() {
    let temp_dir = TempDir::new().expect("temp dir");
    let input = SubmittedInput::from("look @https://example.com/sample.png?cache=1".to_string());

    assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
    assert!(input.attachments.is_empty());
}

#[test]
fn update_input_text_preserves_attachments_for_submit_time_gating() {
    let temp_dir = TempDir::new().expect("temp dir");
    let image = UiContentPart::image("pasted-image", "image/png");
    let mut input = SubmittedInput::new("/compact please inspect this", vec![image.clone()]);

    replace_submitted_input_text(&mut input, "please inspect this".to_string());

    assert_eq!(input.text, "please inspect this");
    assert_eq!(input.attachments, vec![image]);
    assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
}

fn tiny_png_bytes() -> &'static [u8] {
    &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21,
        196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1, 13, 10, 45, 180, 0, 0, 0, 0, 73, 69,
        78, 68, 174, 66, 96, 130,
    ]
}

fn image_payloads(content: &uni::MessageContent) -> Vec<String> {
    let uni::MessageContent::Parts(parts) = content else {
        return Vec::new();
    };
    parts
        .iter()
        .filter_map(|part| match part {
            uni::ContentPart::Image { data, .. } => Some(data.clone()),
            _ => None,
        })
        .collect()
}

fn assert_image_payload_order(content: &uni::MessageContent, expected_prefixes: &[&str]) {
    let payloads = image_payloads(content);
    assert_eq!(payloads.len(), expected_prefixes.len());
    for (payload, expected_prefix) in payloads.iter().zip(expected_prefixes) {
        assert!(payload.starts_with(expected_prefix), "payload {payload:?} should start with {expected_prefix:?}");
    }
}

#[test]
fn test_stalled_follow_up_recovery_prompt_sanitizes_disabled_tools_message() {
    let reason = "Recovery tool-call limit reached after 4 blocked calls. Tools remain disabled while the recovery response is finalized.";
    let prompt = stalled_follow_up_recovery_prompt(reason, false);
    assert!(!prompt.contains("Tools remain disabled"));
    assert!(prompt.contains("Tools are fully enabled for this turn."));
    assert!(prompt.contains("Recovery tool-call limit reached after 4 blocked calls."));
}
