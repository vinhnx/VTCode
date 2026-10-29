use super::super::ToolExecutionRequest;
use super::super::assembly::public_tool_name_candidates;
use super::{
    DispatchMode, ToolRegistry, coerce_string_to_schema_type_in_place, configured_file_operation_max_payload_bytes,
    enforce_file_operation_payload_limit, is_missing_required_arg, normalize_tool_args,
    parse_file_operation_max_payload_bytes, parse_string_as_schema_type, preflight_validate_call,
    preflight_validate_resolved_call, public_exec_validation_args, resolve_dispatch_target,
    resolve_internal_dispatch_tool,
};
use crate::config::constants::tools as tool_names;
use crate::tools::command_args::parse_indexed_command_parts;
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::traits::Tool;
use anyhow::Result;
use serde_json::{Value, json};

async fn new_test_registry() -> (tempfile::TempDir, ToolRegistry) {
    let temp = tempfile::tempdir().expect("temp workspace");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    (temp, registry)
}

#[tokio::test]
async fn internal_dispatch_preserves_normalization_and_candidate_order() {
    let (_temp, registry) = new_test_registry().await;
    for name in [
        "read_file",
        "  ReAd_FiLe  ",
        "functions.ReAd_FiLe",
        "tools.read-file",
        "READ FILE",
        "unknown:READ_FILE",
    ] {
        assert_eq!(resolve_internal_dispatch_tool(&registry, name).as_deref(), Some(tool_names::READ_FILE), "{name}");
        assert_eq!(resolve_dispatch_target(&registry, name, DispatchMode::Harness).unwrap(), tool_names::READ_FILE);
        assert!(resolve_dispatch_target(&registry, name, DispatchMode::ModelPublic).is_err(), "{name}");
    }
    for name in [tool_names::WRITE_FILE, tool_names::EDIT_FILE, tool_names::LIST_FILES] {
        assert_eq!(resolve_internal_dispatch_tool(&registry, name).as_deref(), Some(name));
    }
    assert_eq!(
        resolve_internal_dispatch_tool(&registry, "read_file<|channel|>WRITE_FILE").as_deref(),
        Some(tool_names::WRITE_FILE)
    );
    assert_eq!(
        resolve_internal_dispatch_tool(&registry, "read_file<|channel|>write_file:list_files").as_deref(),
        Some(tool_names::READ_FILE)
    );
}

#[tokio::test]
async fn internal_dispatch_rejects_hidden_tools_outside_allowlist_and_near_misses() {
    let (_temp, registry) = new_test_registry().await;
    assert!(registry.inventory.registration_for(tool_names::GET_ERRORS).is_some());
    for name in [
        "",
        " \t\u{2003}",
        "GET_ERRORS",
        "functions.get_errors",
        "read_files",
        "repo_browser.read_file",
        "READ_FİLE",
    ] {
        assert!(resolve_internal_dispatch_tool(&registry, name).is_none(), "{name:?}");
        assert!(resolve_dispatch_target(&registry, name, DispatchMode::Harness).is_err(), "{name:?}");
        assert!(resolve_dispatch_target(&registry, name, DispatchMode::ModelPublic).is_err(), "{name:?}");
    }
}

#[test]
fn normalize_elides_truncation_only_piped_verifier() {
    // The pipeline exit status belongs to the truncator, so the piped
    // form must never reach validation or execution: the standalone
    // verifier runs instead and its status is truthful.
    let args = json!({"action": "run", "command": "cargo check --locked 2>&1 | head -c 4000"});
    let normalized =
        normalize_tool_args(tool_names::UNIFIED_EXEC, &args, None).expect("rewrite must not fail normalization");
    assert_eq!(normalized.as_ref()["command"], "cargo check --locked 2>&1");
    // The caller's `cmd` spelling is left intact as the typed record.
    assert_eq!(normalized.as_ref()["cmd"], Value::Null);

    let via_cmd = json!({"action": "run", "cmd": "cargo nextest run 2>&1 | tail -15"});
    let via_cmd =
        normalize_tool_args(tool_names::UNIFIED_EXEC, &via_cmd, None).expect("rewrite must not fail normalization");
    assert_eq!(via_cmd.as_ref()["command"], "cargo nextest run 2>&1");
    assert_eq!(via_cmd.as_ref()["cmd"], "cargo nextest run 2>&1 | tail -15");
}

#[test]
fn normalize_leaves_non_rewritable_pipelines_untouched() {
    for command in [
        "cargo check | grep error; true",
        "cargo check && rm -rf target | tail -5",
        "cargo check | tail -5; rm foo.txt",
        "rg -n 'pattern' src | head -20",
        "cargo check --locked",
    ] {
        let args = json!({"action": "run", "command": command});
        let normalized =
            normalize_tool_args(tool_names::UNIFIED_EXEC, &args, None).expect("normalization must succeed");
        assert_eq!(normalized.as_ref()["command"], Value::String(command.to_string()), "must run as typed: {command}");
    }
}

#[test]
fn exec_command_validation_path_rewrites_piped_verifier() {
    let args = json!({"cmd": "cargo check --locked | tail -8"});
    let Some(exec_args) =
        public_exec_validation_args(tool_names::EXEC_COMMAND, &args).expect("validation args must build")
    else {
        panic!("EXEC_COMMAND run must produce exec validation args");
    };
    assert_eq!(exec_args["command"], "cargo check --locked");
}

#[tokio::test]
async fn preflight_accepts_output_limit_for_legacy_strict_schemas() {
    let (_temp, registry) = new_test_registry().await;
    let outcome = preflight_validate_call(
        &registry,
        tool_names::CODE_SEARCH,
        &json!({"query": "ToolRegistry", "max_output_tokens": 37}),
    )
    .expect("a valid integer output limit should pass preflight");
    assert_eq!(outcome.effective_args["max_output_tokens"], 37);
}

#[tokio::test]
async fn preflight_uses_default_output_limit_when_omitted() {
    let (_temp, registry) = new_test_registry().await;
    let outcome = preflight_validate_call(&registry, tool_names::CODE_SEARCH, &json!({"query": "ToolRegistry"}))
        .expect("omitted output limits should remain dispatchable");

    assert_eq!(
        crate::tools::output_limits::max_output_tokens(&outcome.effective_args)
            .expect("default output limit should be valid"),
        vtcode_utility_tool_specs::DEFAULT_MAX_OUTPUT_TOKENS
    );
}

#[tokio::test]
async fn preflight_rejects_invalid_output_limits_before_dispatch() {
    let (_temp, registry) = new_test_registry().await;
    let error = preflight_validate_call(
        &registry,
        tool_names::CODE_SEARCH,
        &json!({"query": "ToolRegistry", "max_output_tokens": "37"}),
    )
    .expect_err("string output limits must be rejected");
    assert!(error.to_string().contains("max_output_tokens must be an integer"));
}

// --- schema-aware string-type coercion -------------------------------------
//
// Regression coverage for the checkpoint-observed defect where the model
// emitted JSON-encoded strings (`"[\"path\"]"`, `"10"`) for array/integer
// fields and retried the same malformed call six times in one turn.

#[test]
fn parse_string_as_schema_type_coerces_array_and_integer() {
    assert_eq!(parse_string_as_schema_type(r#"["path"]"#, "array"), Some(json!(["path"])));
    assert_eq!(parse_string_as_schema_type("10", "integer"), Some(json!(10)));
}

#[test]
fn parse_string_as_schema_type_coerces_boolean_object_and_number() {
    assert_eq!(parse_string_as_schema_type("true", "boolean"), Some(json!(true)));
    assert_eq!(parse_string_as_schema_type("false", "boolean"), Some(json!(false)));
    assert_eq!(parse_string_as_schema_type(r#"{"a": 1}"#, "object"), Some(json!({"a": 1})));
    assert_eq!(parse_string_as_schema_type("3.5", "number"), Some(json!(3.5)));
    // An integer literal is also a valid JSON number.
    assert_eq!(parse_string_as_schema_type("3", "number"), Some(json!(3)));
}

#[test]
fn parse_string_as_schema_type_rejects_float_for_integer() {
    // "10.0" must not be silently coerced to an integer: that would be a
    // lossy reinterpretation. Let strict validation surface the type error.
    assert_eq!(parse_string_as_schema_type("10.0", "integer"), None);
}

#[test]
fn parse_string_as_schema_type_rejects_mismatched_and_malformed() {
    // Parsed type does not match schema type.
    assert_eq!(parse_string_as_schema_type(r#"{"a": 1}"#, "array"), None);
    assert_eq!(parse_string_as_schema_type(r#"["path"]"#, "object"), None);
    assert_eq!(parse_string_as_schema_type("10", "boolean"), None);
    // Malformed JSON stays a string and is left for strict validation.
    assert_eq!(parse_string_as_schema_type("not json", "array"), None);
    assert_eq!(parse_string_as_schema_type("[\"path\"", "array"), None);
    // Unknown schema type is never coerced.
    assert_eq!(parse_string_as_schema_type("anything", "string"), None);
}

#[test]
fn coerce_string_to_schema_type_in_place_only_touches_strings() {
    let mut already_array = json!(["path"]);
    let schema = json!({"type": "array"});
    assert!(!coerce_string_to_schema_type_in_place(&mut already_array, &schema));
    assert_eq!(already_array, json!(["path"]));

    let mut no_type = json!("keep me");
    let schema = json!({}); // no declared type → free-form string
    assert!(!coerce_string_to_schema_type_in_place(&mut no_type, &schema));
    assert_eq!(no_type, json!("keep me"));
}

#[test]
fn normalize_tool_args_coerces_code_search_stringified_args() {
    // Mirrors the exact checkpoint defect: result_types and max_results
    // arrive as JSON-encoded strings.
    let schema = json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "result_types": {"type": "array", "items": {"type": "string", "enum": ["definition", "usage", "text", "path"]}},
            "max_results": {"type": "integer", "minimum": 1, "maximum": 100}
        }
    });
    let args = json!({
        "query": "turn_loop",
        "result_types": "[\"path\"]",
        "max_results": "10"
    });
    let normalized = normalize_tool_args(tool_names::CODE_SEARCH, &args, Some(&schema))
        .expect("code_search stringified args normalize");
    assert_eq!(normalized["result_types"], json!(["path"]));
    assert_eq!(normalized["max_results"], json!(10));
    assert_eq!(normalized["query"], json!("turn_loop"));
}

#[tokio::test]
async fn preflight_coerces_stringified_code_search_args_then_validates() {
    let (_temp, registry) = new_test_registry().await;
    // The exact malformed shape from the checkpoint should now pass preflight.
    let outcome = preflight_validate_call(
        &registry,
        tool_names::CODE_SEARCH,
        &json!({"query": "turn_loop", "result_types": "[\"path\"]", "max_results": "10"}),
    )
    .expect("stringified args should be coerced and pass preflight");
    assert_eq!(outcome.effective_args["result_types"], json!(["path"]));
    assert_eq!(outcome.effective_args["max_results"], json!(10));
}

#[tokio::test]
async fn preflight_coercion_still_enforces_bounds_and_enum() {
    let (_temp, registry) = new_test_registry().await;
    // Coerced to integer 999, then the max=100 bound must still reject it.
    let over_max = preflight_validate_call(
        &registry,
        tool_names::CODE_SEARCH,
        &json!({"query": "turn_loop", "max_results": "999"}),
    )
    .expect_err("coerced integer must still be bounds-checked");
    assert!(over_max.to_string().contains("max_results"), "msg: {over_max}");

    // Coerced to array ["bogus"], then the item enum must still reject it.
    let bad_enum = preflight_validate_call(
        &registry,
        tool_names::CODE_SEARCH,
        &json!({"query": "turn_loop", "result_types": "[\"bogus\"]"}),
    )
    .expect_err("coerced array items must still be enum-checked");
    assert!(bad_enum.to_string().contains("result_types"), "msg: {bad_enum}");
}

#[tokio::test]
async fn preflight_does_not_coerce_max_output_tokens_string() {
    // The output-budget field has a dedicated strict validator that must keep
    // rejecting strings even though schema-aware coercion exists for other
    // integer fields.
    let (_temp, registry) = new_test_registry().await;
    let error =
        preflight_validate_call(&registry, tool_names::CODE_SEARCH, &json!({"query": "x", "max_output_tokens": "100"}))
            .expect_err("max_output_tokens string must remain rejected");
    assert!(error.to_string().contains("max_output_tokens must be an integer"));
}
#[test]
fn patch_action_within_limit_is_allowed() {
    let mut failures = Vec::new();
    let args = json!({
        "action": "patch",
        "patch": "*** Begin Patch\n*** End Patch\n"
    });

    enforce_file_operation_payload_limit(tool_names::UNIFIED_FILE, &args, 1024, &mut failures);
    assert!(failures.is_empty());
}
#[test]
fn patch_action_over_limit_is_rejected() {
    let mut failures = Vec::new();
    let args = json!({
        "action": "patch",
        "patch": "x".repeat(512)
    });

    enforce_file_operation_payload_limit(tool_names::UNIFIED_FILE, &args, 128, &mut failures);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("payload too large"));
    assert!(failures[0].contains("Split the change"));
}
#[test]
fn edit_tool_over_limit_is_rejected() {
    let mut failures = Vec::new();
    let args = json!({
        "path": "file.txt",
        "old_str": "old",
        "new_str": "x".repeat(512)
    });

    enforce_file_operation_payload_limit(tool_names::EDIT_FILE, &args, 128, &mut failures);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("action='edit'"));
}

#[test]
fn read_action_is_not_limited() {
    let mut failures = Vec::new();
    let args = json!({
        "action": "read",
        "path": "README.md"
    });

    enforce_file_operation_payload_limit(tool_names::UNIFIED_FILE, &args, 1, &mut failures);
    assert!(failures.is_empty());
}

#[test]
fn edit_file_required_args_accept_legacy_key_names() {
    let args = json!({
        "path": "file.txt",
        "old_string": "old",
        "new_string": "new"
    });

    assert!(!is_missing_required_arg(tool_names::EDIT_FILE, &args, "path"));
    assert!(!is_missing_required_arg(tool_names::EDIT_FILE, &args, "old_str"));
    assert!(!is_missing_required_arg(tool_names::EDIT_FILE, &args, "new_str"));
}

#[test]
fn parse_payload_limit_accepts_safe_override() {
    let parsed = parse_file_operation_max_payload_bytes(Some("2048"));
    assert_eq!(parsed, Some(2048));
}

#[test]
fn parse_payload_limit_rejects_too_small_values() {
    let parsed = parse_file_operation_max_payload_bytes(Some("512"));
    assert_eq!(parsed, None);
}

#[test]
fn parse_payload_limit_rejects_invalid_values() {
    let parsed = parse_file_operation_max_payload_bytes(Some("not-a-number"));
    assert_eq!(parsed, None);
}

#[test]
fn configured_payload_limit_is_always_safe() {
    let configured = configured_file_operation_max_payload_bytes();
    assert!(configured >= 1024);
}

#[test]
fn apply_patch_required_arg_accepts_input_alias() {
    assert!(!is_missing_required_arg(tool_names::APPLY_PATCH, &json!({"input": ""}), "input"));
}

#[test]
fn apply_patch_required_arg_accepts_raw_string_payload() {
    assert!(!is_missing_required_arg(tool_names::APPLY_PATCH, &json!(""), "input"));
}

#[test]
fn run_pty_cmd_required_arg_accepts_zero_based_indexed_command() -> Result<()> {
    let input = json!({
        "command.0": "ls",
        "command.1": "-a"
    });
    let args = normalize_tool_args(tool_names::RUN_PTY_CMD, &input, None)?;

    assert!(!is_missing_required_arg(tool_names::RUN_PTY_CMD, args.as_ref(), "command"));
    assert_eq!(args.get("command").and_then(|value| value.as_str()), Some("ls -a"));
    Ok(())
}

#[test]
fn run_pty_cmd_required_arg_accepts_one_based_indexed_command() -> Result<()> {
    let input = json!({
        "command.1": "ls",
        "command.2": "-a"
    });
    let args = normalize_tool_args(tool_names::RUN_PTY_CMD, &input, None)?;

    assert!(!is_missing_required_arg(tool_names::RUN_PTY_CMD, args.as_ref(), "command"));
    assert_eq!(args.get("command").and_then(|value| value.as_str()), Some("ls -a"));
    Ok(())
}

#[test]
fn indexed_command_parts_require_zero_or_one_based_sequences() {
    assert_eq!(
        parse_indexed_command_parts(
            json!({
                "command.0": "ls",
                "command.1": "-a"
            })
            .as_object()
            .expect("object"),
        )
        .expect("valid indexed args"),
        Some(vec!["ls".to_string(), "-a".to_string()])
    );
    assert_eq!(
        parse_indexed_command_parts(
            json!({
                "command.1": "ls",
                "command.2": "-a"
            })
            .as_object()
            .expect("object"),
        )
        .expect("valid indexed args"),
        Some(vec!["ls".to_string(), "-a".to_string()])
    );
    assert_eq!(
        parse_indexed_command_parts(json!({"command.2": "ls"}).as_object().expect("object"))
            .expect("valid indexed args"),
        None
    );
}

#[test]
fn tool_name_candidates_extract_channel_suffix_alias() {
    let candidates = public_tool_name_candidates("assistant<|channel|>apply_patch");
    assert!(candidates.iter().any(|c| c == "apply_patch"));
}

#[test]
fn tool_name_candidates_normalize_humanized_name() {
    let candidates = public_tool_name_candidates("Read file");
    assert!(candidates.iter().any(|c| c == "read_file"));
}

#[test]
fn request_user_input_args_accept_details_alias() -> Result<()> {
    let schema = RequestUserInputTool.parameter_schema().expect("request_user_input schema");
    let args = json!({
        "questions": [{
            "id": "scope",
            "header": "Scope",
            "question": "Which direction should we take?",
            "options": [
                {
                    "label": "Minimal",
                    "details": "Ship the smallest viable slice."
                },
                {
                    "label": "Full",
                    "details": "Ship the full implementation."
                }
            ]
        }]
    });

    let normalized = normalize_tool_args(tool_names::REQUEST_USER_INPUT, &args, Some(&schema))?;
    let option = &normalized["questions"][0]["options"][0];
    assert_eq!(option.get("description").and_then(Value::as_str), Some("Ship the smallest viable slice."));
    assert!(option.get("details").is_none());
    Ok(())
}

#[test]
fn task_tracker_args_accept_details_alias() -> Result<()> {
    let schema = json!({
        "type": "object",
        "properties": {
            "action": { "type": "string" },
            "description": { "type": "string" }
        }
    });
    let args = json!({
        "action": "add",
        "details": "Add regression coverage"
    });

    let normalized = normalize_tool_args(tool_names::TASK_TRACKER, &args, Some(&schema))?;
    assert_eq!(normalized.get("description").and_then(Value::as_str), Some("Add regression coverage"));
    assert!(normalized.get("details").is_none());
    Ok(())
}

#[test]
fn details_alias_does_not_shadow_real_details_field() -> Result<()> {
    let schema = json!({
        "type": "object",
        "properties": {
            "description": { "type": "string" },
            "details": { "type": "string" }
        }
    });
    let args = json!({
        "details": "Keep the real details field."
    });

    let normalized = normalize_tool_args(tool_names::TASK_TRACKER, &args, Some(&schema))?;
    assert!(normalized.get("description").is_none());
    assert_eq!(normalized.get("details").and_then(Value::as_str), Some("Keep the real details field."));
    Ok(())
}

#[tokio::test]
async fn command_session_preflight_rejects_run_without_command() {
    let (_temp, registry) = new_test_registry().await;

    let err = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({"action": "run"}))
        .expect_err("missing command should fail preflight");

    assert!(err.to_string().contains("Missing required argument: command"));
}

#[tokio::test]
async fn exec_command_preflight_preserves_public_name_and_validates_as_run() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;

    let result = preflight_validate_call(
        &registry,
        tool_names::EXEC_COMMAND,
        &json!({"cmd": "rg --files", "workdir": ".", "tty": true}),
    )?;

    assert_eq!(result.normalized_tool_name, tool_names::EXEC_COMMAND);
    assert_eq!(result.effective_args["action"], "run");
    assert_eq!(result.effective_args["command"], "rg --files");
    assert_eq!(result.effective_args["workdir"], ".");
    assert_eq!(result.effective_args["tty"], true);
    assert!(result.readonly_classification);
    Ok(())
}

#[tokio::test]
async fn planning_preflight_rejects_mutating_suffixes_and_conflicting_raw_aliases() {
    let (_temp, registry) = new_test_registry().await;
    registry.enable_planning();
    for args in [
        json!({"cmd":"cargo check | sort", "args":["-o", "changed.txt"]}),
        json!({"cmd":"cat README.md", "raw_command":"printf changed > changed.txt"}),
    ] {
        let error = preflight_validate_call(&registry, tool_names::EXEC_COMMAND, &args)
            .expect_err("mutating invocation cannot acquire planning admission");
        assert_eq!(error.to_string(), super::agent_execution::PLANNING_DENIED_CONTEXT);
    }
    let safe =
        preflight_validate_call(&registry, tool_names::EXEC_COMMAND, &json!({"cmd":"sort", "args":["README.md"]}))
            .expect("safe appended operands remain readable in planning");
    assert!(safe.readonly_classification);
    assert!(safe.parallel_safe_after_preflight);
}

#[tokio::test]
async fn planning_preflight_allows_checkpoint_style_readonly_inspection() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;
    registry.enable_planning();
    let command = r#"sed -n '180,285p' src/main.rs; sed -n '60,285p' src/startup/mod.rs; sed -n '1,220p' src/main_helpers/bootstrap.rs; rg -n "\[profile|lto|codegen-units|strip" Cargo.toml"#;

    let result = preflight_validate_call(
        &registry,
        tool_names::EXEC_COMMAND,
        &json!({
            "cmd": command,
            "workdir": ".",
            "yield_time_ms": 10000,
            "max_output_tokens": 30000
        }),
    )?;

    assert!(result.readonly_classification);
    assert_eq!(result.effective_args["action"], "run");
    Ok(())
}

#[tokio::test]
async fn exec_command_preflight_rejects_dangerous_command() {
    let (_temp, registry) = new_test_registry().await;

    let err = preflight_validate_call(&registry, tool_names::EXEC_COMMAND, &json!({"cmd": "git reset --hard HEAD~1"}))
        .expect_err("dangerous exec_command should fail preflight");

    let text = err.to_string();
    assert!(text.contains("Tool preflight validation failed for 'exec_command'"));
    assert!(text.contains("Command security check failed"));
}

#[tokio::test]
async fn exec_command_approval_required_payload_is_seen_as_shell_run() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;
    let args = json!({
        "cmd": "cargo check",
        "sandbox_permissions": "require_escalated",
        "justification": "Need unsandboxed access for this check."
    });

    let reason = registry
        .shell_run_approval_reason(tool_names::EXEC_COMMAND, Some(&args))
        .await?;

    assert!(
        reason
            .as_deref()
            .is_some_and(|text| text.contains("without sandbox restrictions"))
    );
    Ok(())
}

#[tokio::test]
async fn direct_unsandboxed_exec_requires_operator_preapproval() {
    let (_temp, registry) = new_test_registry().await;
    let args = json!({
        "cmd": "printf guarded",
        "sandbox_permissions": "require_escalated",
        "justification": "Need unsandboxed access for this check."
    });

    let outcome = registry
        .execute_public_tool_request(ToolExecutionRequest::new(tool_names::EXEC_COMMAND, args))
        .await;
    let error = outcome.error.expect("direct escalation must be rejected");
    assert!(error.message.contains("requires an enforced operator approval decision"));
}

#[tokio::test]
async fn write_stdin_preflight_uses_session_write_validation() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;

    let result = preflight_validate_call(
        &registry,
        tool_names::WRITE_STDIN,
        &json!({
            "session_id": "run-1",
            "chars": "git reset --hard HEAD~1\n"
        }),
    )?;

    assert_eq!(result.normalized_tool_name, tool_names::WRITE_STDIN);
    assert_eq!(result.effective_args["action"], "write");
    assert_eq!(result.effective_args["input"], "git reset --hard HEAD~1\n");
    assert!(!result.readonly_classification);
    Ok(())
}

#[tokio::test]
async fn write_stdin_preflight_uses_session_poll_validation() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;

    let result = preflight_validate_call(
        &registry,
        tool_names::WRITE_STDIN,
        &json!({
            "session_id": "run-1",
            "chars": "",
            "yield_time_ms": 25,
            "max_output_tokens": 7
        }),
    )?;

    assert_eq!(result.normalized_tool_name, tool_names::WRITE_STDIN);
    assert_eq!(result.effective_args["action"], "poll");
    assert!(result.effective_args.get("input").is_none());
    assert_eq!(result.effective_args["yield_time_ms"], 25);
    assert_eq!(result.effective_args["max_output_tokens"], 7);
    assert!(result.readonly_classification);
    Ok(())
}

#[tokio::test]
async fn write_stdin_poll_preflight_rejects_non_string_session_id() {
    let (_temp, registry) = new_test_registry().await;

    let error = preflight_validate_call(&registry, tool_names::WRITE_STDIN, &json!({"session_id": 1, "chars": ""}))
        .expect_err("non-string session id should fail preflight");

    assert!(error.to_string().contains("write_stdin"));
    assert!(error.to_string().contains("Missing required argument: session_id"));
}

#[tokio::test]
async fn command_session_preflight_rejects_missing_action_without_inferable_args() {
    let (_temp, registry) = new_test_registry().await;

    let err = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({}))
        .expect_err("missing action should fail preflight");

    assert!(err.to_string().contains(&format!(
        "Invalid arguments for tool '{}': missing action; provide `action` or inferable exec arguments",
        tool_names::UNIFIED_EXEC
    )));
}

#[tokio::test]
async fn command_session_preflight_rejects_write_without_input() {
    let (_temp, registry) = new_test_registry().await;

    let err = preflight_validate_resolved_call(
        &registry,
        tool_names::UNIFIED_EXEC,
        &json!({"action": "write", "session_id": "run-1"}),
    )
    .expect_err("missing input should fail preflight");

    assert!(err.to_string().contains("Missing required argument: input or chars or text"));
}

#[tokio::test]
async fn command_session_preflight_rejects_poll_without_session_id() {
    let (_temp, registry) = new_test_registry().await;

    let err = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({"action": "poll"}))
        .expect_err("missing session_id should fail preflight");

    assert!(err.to_string().contains("Missing required argument: session_id"));
}

#[tokio::test]
async fn command_session_preflight_accepts_list_without_extra_args() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;

    let result = preflight_validate_resolved_call(&registry, tool_names::UNIFIED_EXEC, &json!({"action": "list"}))?;

    assert_eq!(result.normalized_tool_name, tool_names::UNIFIED_EXEC);
    Ok(())
}

#[tokio::test]
async fn command_session_preflight_accepts_inspect_with_spool_path() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;

    let result = preflight_validate_resolved_call(
        &registry,
        tool_names::UNIFIED_EXEC,
        &json!({"action": "inspect", "spool_path": ".vtcode/context/tool_outputs/out.log"}),
    )?;

    assert_eq!(result.normalized_tool_name, tool_names::UNIFIED_EXEC);
    Ok(())
}

#[tokio::test]
async fn file_operation_command_payload_preflight_remaps_to_command_session() -> Result<()> {
    let (_temp, registry) = new_test_registry().await;

    let result = preflight_validate_resolved_call(
        &registry,
        tool_names::UNIFIED_FILE,
        &json!({
            "command": "echo vtcode",
            "cwd": ".",
        }),
    )?;

    assert_eq!(result.normalized_tool_name, tool_names::UNIFIED_EXEC);
    assert_eq!(result.effective_args["action"], "run");
    assert_eq!(result.effective_args["command"], "echo vtcode");
    assert_eq!(result.effective_args["cwd"], ".");
    Ok(())
}
