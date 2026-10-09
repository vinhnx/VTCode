//! Tool intent classification: mutating vs read-only, parallel-safe, and
//! action extraction for unified tool surfaces.

pub mod actions;
pub mod activity;
pub mod classify;
pub mod readonly;
pub mod types;

pub use actions::{
    action_qualified_policy_name, command_session_action, command_session_action_in, command_session_action_is,
    file_operation_action, file_operation_action_in, file_operation_action_is, mcp_action, mcp_action_is,
};
pub use activity::{
    GENERIC_VERIFIER_DESCRIPTION, ShellActivity, VERIFIER_SHELL_FORM_NOTE, classify_shell_activity,
    default_verifier_for_workspace, rewrite_truncation_only_verifier, shell_args_as_executed,
    shell_command_contains_verifier, shell_command_is_admitted_verification_attempt, shell_command_is_output_search,
    shell_command_is_standalone_grep_search, verification_recovery_directive, verifier_reference,
};
pub use classify::{
    builtin_tool_behavior, canonical_command_session_tool_name, classify_tool_intent, is_command_run_tool,
    is_command_run_tool_call, is_command_session_tool, is_command_tool, is_edited_file_conflict_guarded_call,
    is_exec_session_cleanup_call, is_parallel_safe_call, is_parallel_safe_call_with_intent, is_turn_budget_exempt_call,
    planning_allowed_actions, remap_file_operation_command_args_to_command_session, should_use_spool_reference_only,
};
pub use readonly::{is_readonly_command_session_command, is_spool_file_read_command};
pub use types::{ToolBehavior, ToolIntent, ToolIntentClassifier, ToolMutationModel, ToolSurfaceKind};

#[cfg(test)]
mod tests {
    use super::{
        canonical_command_session_tool_name, classify_tool_intent, file_operation_action, is_command_run_tool_call,
        is_command_session_tool, is_edited_file_conflict_guarded_call, is_parallel_safe_call,
        is_parallel_safe_call_with_intent, remap_file_operation_command_args_to_command_session,
        should_use_spool_reference_only,
    };
    use crate::config::constants::tools;
    use serde_json::json;

    #[test]
    fn file_operation_read_is_retry_safe() {
        let intent = classify_tool_intent(tools::UNIFIED_FILE, &json!({"action": "read", "path": "README.md"}));
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_poll_is_retry_safe() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "poll", "session_id": 1}));
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn turn_budget_exempt_covers_wait_and_inspect_only() {
        use super::is_turn_budget_exempt_call;

        // Exec-family wait/inspect calls are exempt.
        assert!(is_turn_budget_exempt_call(tools::UNIFIED_EXEC, &json!({"action": "wait", "session_id": "run-1"})));
        assert!(is_turn_budget_exempt_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "inspect", "spool_path": ".vtcode/context/tool_outputs/run-1.txt"})
        ));
        assert!(is_turn_budget_exempt_call(tools::WRITE_STDIN, &json!({"action": "wait", "session_id": "run-1"})));
        assert!(is_turn_budget_exempt_call(tools::EXEC_COMMAND, &json!({"action": "wait", "session_id": "run-1"})));

        // Work-producing exec calls are NOT exempt.
        assert!(!is_turn_budget_exempt_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "./scripts/check-dev.sh"})
        ));
        assert!(!is_turn_budget_exempt_call(tools::UNIFIED_EXEC, &json!({"action": "poll", "session_id": "run-1"})));
        assert!(!is_turn_budget_exempt_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1", "input": "y"})
        ));
        assert!(!is_turn_budget_exempt_call(tools::UNIFIED_EXEC, &json!({"action": "list"})));

        // Non-exec tools are never exempt, even with matching action strings.
        assert!(!is_turn_budget_exempt_call(tools::READ_FILE, &json!({"action": "wait", "path": "x.txt"})));
        assert!(!is_turn_budget_exempt_call(tools::APPLY_PATCH, &json!({"action": "inspect", "input": "..."})));
        assert!(!is_turn_budget_exempt_call(tools::UNIFIED_FILE, &json!({"action": "read", "path": "x.txt"})));

        // Alias surface names normalize to the canonical exec surface.
        assert!(is_turn_budget_exempt_call(tools::RUN_PTY_CMD, &json!({"action": "wait", "session_id": "run-1"})));
    }

    #[test]
    fn skill_loading_is_mutating_and_requires_confirmation() {
        let intent = classify_tool_intent(tools::LOAD_SKILL, &json!({"name": "example"}));
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn command_session_inspect_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "inspect", "spool_path": ".vtcode/context/tool_outputs/run-1.txt"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_continue_without_input_is_retry_safe() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "continue", "session_id": "run-1"}));
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_continue_with_input_is_mutating_and_destructive() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1", "input": "q"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn command_session_continue_with_empty_input_stays_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1", "input": ""}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_is_mutating_and_destructive() {
        let intent =
            classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": "rm -rf /tmp/test"}));
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn command_session_run_echo_is_read_only() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": "echo hi"}));
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_diff_is_read_only() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": "diff a.rs b.rs"}));
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_find_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "find . -type f -name '*.rs' -not -path '*/target/*'"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_grep_wc_head_are_read_only() {
        for cmd in [
            "grep -rn 'todo' src",
            "wc -l src/main.rs",
            "head -50 src/lib.rs",
            "tail -20 src/lib.rs",
            "sort src/words.txt | uniq",
            "ast-grep -p 'foo($A)' -l rs",
        ] {
            let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": cmd}));
            assert!(!intent.mutating, "expected '{cmd}' to be read-only");
            assert!(intent.readonly_unified_action, "expected '{cmd}' to be readonly_unified_action");
        }
    }

    #[test]
    fn command_session_run_with_redirection_is_mutating() {
        for cmd in [
            "cat a.txt > b.txt",
            "grep x src > out.txt",
            "diff a b | wc -l > count.txt",
            "echo $(date) > log.txt",
        ] {
            let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": cmd}));
            assert!(intent.mutating, "expected '{cmd}' to be mutating because it contains redirection/substitution");
        }
    }

    #[test]
    fn command_session_run_find_with_destructive_flags_is_mutating() {
        for cmd in [
            "find . -type f -delete",
            "find . -name '*.tmp' -exec rm {} \\;",
            "find . -name '*.tmp' -exec chmod 600 {} \\;",
        ] {
            let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": cmd}));
            assert!(intent.mutating, "expected '{cmd}' to be mutating because it has destructive find flags");
        }
    }

    #[test]
    fn command_session_run_pipelines_with_unsafe_segments_are_mutating() {
        for cmd in ["cat a.txt | tee b.txt", "python script.py | cat", "grep x src | rm -rf"] {
            let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": cmd}));
            assert!(intent.mutating, "expected '{cmd}' to be mutating because a pipeline segment is unsafe");
        }
    }

    #[test]
    fn command_session_run_allowlisted_is_read_only() {
        let intent =
            classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": "rg planning_active src"}));
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_dry_run_is_read_only() {
        let intent =
            classify_tool_intent(tools::UNIFIED_EXEC, &json!({"action": "run", "command": "npm install --dry-run"}));
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn parallel_safe_calls_reject_control_and_exec_paths() {
        assert!(is_parallel_safe_call(tools::READ_FILE, &json!({"path": "README.md"})));
        assert!(!is_parallel_safe_call(tools::LIST_PTY_SESSIONS, &json!({})));
        assert!(!is_parallel_safe_call(tools::REQUEST_USER_INPUT, &json!({"questions": []})));
        assert!(!is_parallel_safe_call(tools::UNIFIED_EXEC, &json!({"action": "inspect", "session_id": "run-1"})));
        assert!(is_parallel_safe_call(tools::EXEC_COMMAND, &json!({"cmd": "sed -n '1,40p' README.md"})));
        assert!(!is_parallel_safe_call(tools::EXEC_COMMAND, &json!({"cmd": "cat README.md && cat Cargo.toml"})));
        assert!(!is_parallel_safe_call(
            tools::EXEC_COMMAND,
            &json!({"cmd": "sed -n '1,40p' README.md", "tty": true})
        ));
    }

    #[test]
    fn parallel_safe_with_intent_matches_classifying_variant() {
        // The with-intent variant must agree with the classifying variant for
        // every branch: command-run payloads, non-command builtin behaviors,
        // and unknown tools that fall back to plain intent classification.
        for (tool_name, args) in [
            (tools::UNIFIED_EXEC, json!({"action": "run", "command": "rg -n 'todo' src"})),
            (tools::UNIFIED_EXEC, json!({"action": "run", "command": "rm -rf /tmp/test"})),
            (tools::UNIFIED_EXEC, json!({"action": "poll", "session_id": "run-1"})),
            (tools::EXEC_COMMAND, json!({"cmd": "sed -n '1,40p' README.md"})),
            (tools::READ_FILE, json!({"path": "README.md"})),
            (tools::WRITE_FILE, json!({"path": "README.md", "content": "x"})),
            (tools::APPLY_PATCH, json!({"input": "*** Begin Patch\n*** End Patch\n"})),
            ("totally_unknown_tool", json!({"path": "README.md"})),
        ] {
            let with_intent =
                is_parallel_safe_call_with_intent(tool_name, &args, &classify_tool_intent(tool_name, &args));
            assert_eq!(
                with_intent,
                is_parallel_safe_call(tool_name, &args),
                "variants disagree for {tool_name} with {args}"
            );
        }
    }

    #[test]
    fn command_session_cmd_alias_infers_run() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"cmd": "rm -rf /tmp/test"}));
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn command_session_chars_alias_infers_write() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"session_id": "abc123", "chars": "status\n"}));
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn write_stdin_empty_chars_is_read_only_and_retry_safe() {
        let intent = classify_tool_intent(tools::WRITE_STDIN, &json!({"session_id": "abc123", "chars": ""}));

        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(!intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn write_stdin_non_empty_chars_is_mutating() {
        let intent = classify_tool_intent(tools::WRITE_STDIN, &json!({"session_id": "abc123", "chars": "  status\n"}));

        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn command_session_text_alias_infers_write() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"session_id": "abc123", "text": "status\n"}));
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn command_session_spool_path_alias_infers_inspect() {
        let intent =
            classify_tool_intent(tools::UNIFIED_EXEC, &json!({"spool_path": ".vtcode/context/tool_outputs/run-1.txt"}));
        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(intent.readonly_unified_action);
    }

    #[test]
    fn command_session_compact_session_alias_infers_poll() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"s": "run-1"}));
        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(intent.readonly_unified_action);
    }

    #[test]
    fn file_operation_unknown_args_require_action() {
        let args = json!({
            "unexpected": true
        });
        let action = file_operation_action(&args);
        assert_eq!(action, None);
    }

    #[test]
    fn file_operation_compact_path_alias_infers_read() {
        let args = json!({
            "p": "README.md"
        });
        let action = file_operation_action(&args);
        assert_eq!(action, Some("read"));
    }

    #[test]
    fn remap_file_operation_command_args_maps_command_payload_to_command_session() {
        let remapped = remap_file_operation_command_args_to_command_session(&json!({
            "command": "cargo check",
            "cwd": ".",
            "timeout_ms": 1000
        }))
        .expect("command payload should remap");

        assert_eq!(remapped["action"], "run");
        assert_eq!(remapped["command"], "cargo check");
        assert_eq!(remapped["cwd"], ".");
        assert_eq!(remapped["timeout_ms"], 1000);
    }

    #[test]
    fn remap_file_operation_command_args_accepts_exec_action_aliases() {
        let remapped = remap_file_operation_command_args_to_command_session(&json!({
            "action": "shell",
            "cmd": "echo ok"
        }))
        .expect("shell action alias should remap");

        assert_eq!(remapped["action"], "run");
        assert_eq!(remapped["command"], "echo ok");
    }

    #[test]
    fn remap_file_operation_command_args_rejects_non_command_actions() {
        let remapped = remap_file_operation_command_args_to_command_session(&json!({
            "action": "read",
            "command": "echo ok"
        }));

        assert_eq!(remapped, None);
    }

    #[test]
    fn edited_file_conflict_guard_accepts_supported_mutations() {
        assert!(is_edited_file_conflict_guarded_call(
            tools::WRITE_FILE,
            &json!({"path": "README.md", "content": "agent"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::CREATE_FILE,
            &json!({"path": "README.md", "content": "agent"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::EDIT_FILE,
            &json!({"path": "README.md", "old_str": "a", "new_str": "b"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::APPLY_PATCH,
            &json!({"patch": "*** Begin Patch\n*** End Patch\n"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "write", "path": "README.md", "content": "agent"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "create", "path": "README.md", "content": "agent"})
        ));
    }

    #[test]
    fn edited_file_conflict_guard_rejects_non_guarded_calls() {
        assert!(!is_edited_file_conflict_guarded_call(tools::READ_FILE, &json!({"path": "README.md"})));
        assert!(!is_edited_file_conflict_guarded_call(tools::GREP_FILE, &json!({"pattern": "needle", "path": "."})));
        assert!(!is_edited_file_conflict_guarded_call(tools::LIST_FILES, &json!({"path": "."})));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "read", "path": "README.md"})
        ));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "delete", "path": "README.md"})
        ));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "git status"})
        ));
    }

    #[test]
    fn canonical_command_session_tool_name_normalizes_exec_aliases() {
        for alias in [
            tools::UNIFIED_EXEC,
            tools::RUN_PTY_CMD,
            tools::SEND_PTY_INPUT,
            tools::READ_PTY_SESSION,
            tools::LIST_PTY_SESSIONS,
            tools::CLOSE_PTY_SESSION,
            tools::EXECUTE_CODE,
            tools::EXEC_PTY_CMD,
            tools::EXEC_COMMAND,
            tools::WRITE_STDIN,
            tools::SHELL,
            "bash",
            "exec",
            "container.exec",
        ] {
            assert_eq!(canonical_command_session_tool_name(alias), Some(tools::UNIFIED_EXEC));
            assert!(is_command_session_tool(alias));
        }
        assert!(!is_command_session_tool(tools::READ_FILE));
        assert!(!is_command_session_tool(""));
    }

    #[test]
    fn spool_reference_only_detects_exec_aliases() {
        assert!(should_use_spool_reference_only(
            Some(tools::RUN_PTY_CMD),
            &json!({"spool_path": ".vtcode/context/tool_outputs/run-1.txt"})
        ));
    }

    #[test]
    fn spool_reference_only_detects_exec_payload_without_tool_name() {
        assert!(should_use_spool_reference_only(
            None,
            &json!({
                "command": "cargo check",
                "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
                "exit_code": 0
            })
        ));
    }

    #[test]
    fn spool_reference_only_skips_loop_recovery_payloads() {
        assert!(!should_use_spool_reference_only(
            Some(tools::UNIFIED_EXEC),
            &json!({
                "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
                "exit_code": 0,
                "loop_detected": true
            })
        ));
    }

    #[test]
    fn is_command_run_tool_call_only_accepts_run_actions() {
        assert!(is_command_run_tool_call(tools::RUN_PTY_CMD, &json!({"command": "cargo check"})));
        assert!(is_command_run_tool_call(tools::UNIFIED_EXEC, &json!({"action": "run", "command": "cargo check"})));
        assert!(is_command_run_tool_call(tools::EXEC_COMMAND, &json!({"cmd": "cargo check"})));
        assert!(!is_command_run_tool_call(tools::UNIFIED_EXEC, &json!({"action": "poll", "session_id": "run-1"})));
        assert!(!is_command_run_tool_call(tools::WRITE_STDIN, &json!({"session_id": "run-1", "chars": "q"})));
    }
}
