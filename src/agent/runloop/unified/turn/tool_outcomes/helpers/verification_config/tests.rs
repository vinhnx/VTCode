use super::*;
use serde_json::{Value, json};

const LINT: &str = "npx --no-install markdownlint-cli2 README.md";
const NEW_LINT: &str = "npx --no-install markdownlint-cli2 docs/guide.md";

fn append_call(history: &mut Vec<uni::Message>, id: &str, name: &str, args: Value, output: Value) {
    history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            id.to_string(),
            name.to_string(),
            args.to_string(),
        )],
    ));
    history.push(uni::Message::tool_response(id.to_string(), output.to_string()));
}

fn docs_history() -> Vec<uni::Message> {
    let mut history = vec![uni::Message::user("fix README alignment".to_string())];
    append_call(
        &mut history,
        "docs",
        "write_file",
        json!({"path":"README.md", "content":"text"}),
        json!({"success":true}),
    );
    history
}

fn running_output(session_id: &str) -> Value {
    // Use the same projection as shipped model-visible exec results: the
    // top-level ID disappears and the advertised continuation retains it.
    let serialized = crate::agent::runloop::unified::turn::tool_outcomes::response_content::maybe_inline_spooled(
        "exec_command",
        &json!({"success":true, "session_id":session_id, "is_exited":false,
            "next_continue_args":{"session_id":session_id, "action":"poll"},
            "next_wait_args":{"session_id":session_id, "action":"wait", "wait_timeout_seconds":10}}),
    );
    let output: Value = serde_json::from_str(&serialized).expect("projected output");
    assert!(output.get("session_id").is_none());
    output
}

#[test]
fn async_docs_verifier_follows_advertised_session_to_terminal_failure() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("Cargo marker");
    for output in [
        json!({"session_id":"run-docs"}),
        running_output("run-docs"),
        json!({"next_continue_args":{"s":"run-docs", "action":"poll"}}),
    ] {
        for (name, args) in [
            ("write_stdin", json!({"session_id":"run-docs", "chars":""})),
            ("write_stdin", json!({"s":"run-docs", "action":"wait"})),
            (
                vtcode_core::config::constants::tools::UNIFIED_EXEC,
                json!({"session_id":"run-docs", "action":"inspect"}),
            ),
        ] {
            let mut history = docs_history();
            append_call(&mut history, "launch", "exec_command", json!({"cmd":LINT}), output.clone());
            assert_eq!(failed_docs_verifier(&history), None, "running is not a verdict");
            append_call(&mut history, "poll", name, args.clone(), json!({"output":"still running"}));
            assert_eq!(failed_docs_verifier(&history), None);
            append_call(&mut history, "poll", name, args.clone(), json!({"exit_code":1, "stderr":"MD060"}));
            assert_eq!(failed_docs_verifier(&history).as_deref(), Some(LINT), "{name}: {args}, launch: {output}");
            assert_eq!(
                resolve_harness_verifier_command(None, workspace.path(), &history).as_deref(),
                Some(LINT),
                "async Markdown failure must not fall back to Cargo"
            );
            // Terminal completion retires session identity; a reused poll ID
            // cannot overwrite its failure with another session's success.
            append_call(&mut history, "poll", name, args, json!({"exit_code":0}));
            assert_eq!(failed_docs_verifier(&history).as_deref(), Some(LINT));
        }
    }
}

#[test]
fn async_docs_verifier_ignores_unrelated_polls_and_retires_success() {
    let mut history = docs_history();
    append_call(&mut history, "launch", "exec_command", json!({"cmd":LINT}), running_output("run-docs"));
    append_call(
        &mut history,
        "poll",
        "write_stdin",
        json!({"session_id":"run-other", "action":"wait"}),
        json!({"exit_code":1}),
    );
    assert_eq!(failed_docs_verifier(&history), None);
    append_call(
        &mut history,
        "poll",
        "write_stdin",
        json!({"session_id":"run-docs", "action":"wait"}),
        json!({"session_id":"run-other", "exit_code":1}),
    );
    assert_eq!(failed_docs_verifier(&history), None, "mismatched result identity");
    append_call(
        &mut history,
        "poll",
        "write_stdin",
        json!({"session_id":"run-docs", "action":"wait"}),
        json!({"exit_code":0}),
    );
    assert_eq!(failed_docs_verifier(&history), None);
    append_call(
        &mut history,
        "poll",
        "write_stdin",
        json!({"session_id":"run-docs", "action":"wait"}),
        json!({"exit_code":1}),
    );
    assert_eq!(failed_docs_verifier(&history), None, "terminal sessions must be retired");
}

#[test]
fn async_docs_verifier_retires_other_observations_in_the_same_batch() {
    let mut history = docs_history();
    append_call(&mut history, "launch", "exec_command", json!({"cmd":LINT}), running_output("run-docs"));
    let args = json!({"session_id":"run-docs", "action":"wait"}).to_string();
    history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![
            uni::ToolCall::function("first".to_string(), "write_stdin".to_string(), args.clone()),
            uni::ToolCall::function("late".to_string(), "write_stdin".to_string(), args),
        ],
    ));
    history.push(uni::Message::tool_response("first".to_string(), json!({"exit_code":1}).to_string()));
    history.push(uni::Message::tool_response("late".to_string(), json!({"exit_code":0}).to_string()));
    assert_eq!(failed_docs_verifier(&history).as_deref(), Some(LINT));
}

#[test]
fn async_docs_verifier_preserves_rejected_polls_but_discards_lost_or_closed_sessions() {
    for (name, args, output, remains_live) in [
        (
            "write_stdin",
            json!({"session_id":"run-docs", "action":"wait"}),
            json!({"blocked":true, "exit_code":1}),
            true,
        ),
        ("write_stdin", json!({"session_id":"run-docs", "action":"wait"}), json!({"not_executed":true}), true),
        ("write_stdin", json!({"session_id":"run-docs", "action":"wait"}), json!({"cancelled":true}), true),
        (
            "write_stdin",
            json!({"session_id":"run-docs", "action":"close"}),
            json!({"error":"permission denied"}),
            true,
        ),
        (
            "write_stdin",
            json!({"session_id":"run-docs", "action":"wait"}),
            json!({"error":"exec session 'run-docs' not found"}),
            false,
        ),
        ("write_stdin", json!({"session_id":"run-docs", "action":"close"}), json!({"success":true}), false),
        (
            "write_stdin",
            json!({"session_id":"run-docs", "chars":"change command\n"}),
            json!({"exit_code":1}),
            false,
        ),
    ] {
        let mut history = docs_history();
        append_call(&mut history, "launch", "exec_command", json!({"cmd":LINT}), running_output("run-docs"));
        append_call(&mut history, "poll", name, args, output);
        assert_eq!(failed_docs_verifier(&history), None);
        append_call(
            &mut history,
            "poll",
            "write_stdin",
            json!({"session_id":"run-docs", "action":"wait"}),
            json!({"exit_code":1}),
        );
        assert_eq!(failed_docs_verifier(&history).as_deref(), remains_live.then_some(LINT));
    }
}

#[test]
fn async_docs_verifier_supersession_and_fresh_request_are_boundaries() {
    for (new_output, expected) in [
        (json!({"exit_code":0}), None),
        (running_output("run-new"), Some(NEW_LINT)),
    ] {
        let mut history = docs_history();
        append_call(&mut history, "launch", "exec_command", json!({"cmd":LINT}), running_output("run-old"));
        append_call(&mut history, "launch", "exec_command", json!({"cmd":NEW_LINT}), new_output);
        append_call(
            &mut history,
            "poll",
            "write_stdin",
            json!({"session_id":"run-old", "action":"wait"}),
            json!({"exit_code":1}),
        );
        assert_eq!(failed_docs_verifier(&history), None, "old failures must not supersede a newer checker");
        append_call(
            &mut history,
            "poll",
            "write_stdin",
            json!({"session_id":"run-new", "action":"wait"}),
            json!({"exit_code":1}),
        );
        assert_eq!(failed_docs_verifier(&history).as_deref(), expected);
        history.push(uni::Message::user("fix a different document".to_string()));
        append_call(
            &mut history,
            "poll",
            "write_stdin",
            json!({"session_id":"run-new", "action":"wait"}),
            json!({"exit_code":1}),
        );
        assert_eq!(failed_docs_verifier(&history), None, "request-local evidence only");
    }
}
