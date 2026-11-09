//! Concise human summaries for tool transcript rows (CLI parity, no JSON dumps).
//!
//! Consumes the shared headline/detail builders (`vtcode_commons::tool_headlines`)
//! so both surfaces phrase identically: the row title carries the headline,
//! [`activity_output_text`] carries captured output, and [`activity_details_summary`]
//! carries only the remaining parameters the title does not already show. Transcript
//! bodies never carry raw JSON dumps; the agent core still receives full arguments and
//! results through `ThreadEvent` — this module only shapes the user-visible projection.
//!
//! Tool-name knowledge lives in `activity_verbs.rs` (session titles, categories); this
//! module only consumes its answers, so unknown names fall through to generic details
//! and a registry addition can never break the summary.
//! Detail lines are English literals: new user-visible strings stay untranslated unless
//! full native-quality translations ship for every locale (same rule as the activity
//! verbs in `activity_verbs.rs`).

use std::path::Path;

use vtcode_commons::tool_headlines::{collect_param_details, exec_session_param_detail};
use vtcode_exec_events::{ThreadItemDetails, ToolInvocationItem};

use super::super::output_sections::human_output_option;
use super::activity_verbs::{ToolCategory, extra_gist_keys, session_tool_title};

/// User-facing detail lines for a tool row, joined with `\n`.
///
/// Empty when the title already says everything (run commands, plain reads) or when the
/// row's content lives in [`activity_output_text`] instead. Every match arm names the
/// exact human lines it emits — no arm may serialize a `ThreadItemDetails` payload.
pub(crate) fn activity_details_summary(details: &ThreadItemDetails, workspace_root: Option<&Path>) -> String {
    tool_detail_lines(details, workspace_root).join("\n")
}

/// Captured output content for a tool row, when the variant carries any.
///
/// Drives both the expanded transcript tail and the Output inspector tab. MCP results
/// and web-search hits join the command/tool outputs so their content stays reachable
/// now that the JSON dump is gone.
///
/// Postcondition: the returned text is the human projection — core's `Structured
/// output:` metadata envelope is stripped from the captures core annotates
/// (command/tool outputs; only `spool_note` prose survives), MCP results and
/// search hits pass through verbatim since they never carry the envelope, and
/// blank captures collapse to `None`, so `has_output` never offers a tab or
/// deep link that lands on nothing.
pub(crate) fn activity_output_text(details: &ThreadItemDetails) -> Option<String> {
    let raw = match details {
        ThreadItemDetails::CommandExecution(command) => human_output_option(&command.aggregated_output),
        ThreadItemDetails::ToolOutput(output) => human_output_option(&output.output),
        ThreadItemDetails::McpToolCall(call) => call.result.clone(),
        ThreadItemDetails::WebSearch(search) => search
            .results
            .as_ref()
            .filter(|results| !results.is_empty())
            .map(|results| results.join("\n\n")),
        ThreadItemDetails::ToolInvocation(_)
        | ThreadItemDetails::Harness(_)
        | ThreadItemDetails::Error(_)
        | ThreadItemDetails::AgentMessage(_)
        | ThreadItemDetails::Plan(_)
        | ThreadItemDetails::Reasoning(_)
        | ThreadItemDetails::Decision(_)
        | ThreadItemDetails::FileChange(_) => None,
    }?;
    (!raw.trim().is_empty()).then_some(raw)
}

/// Build tool-row body text preserving the status-first-line contract.
///
/// `ActivityState::from_status` parses the first line; every producer routes through
/// here so the shape can never drift per call site. Empty payloads collapse to the
/// bare status instead of trailing blank lines. Multi-line labels degrade the way
/// they always have: only the first line classifies.
pub(crate) fn activity_text(status: &str, payload: &str) -> String {
    if payload.is_empty() {
        status.to_string()
    } else {
        format!("{status}\n\n{payload}")
    }
}

/// Extract the payload below the status line, tolerating legacy shapes.
///
/// Splits on the first newline and drops blank separators, so both
/// `status\n\npayload` and bare `status` (or single-`\n` merges) yield the payload
/// or `""`.
pub(crate) fn activity_payload(text: &str) -> &str {
    text.split_once('\n').map_or("", |(_, rest)| rest.trim_start_matches('\n'))
}

/// Detail lines per event variant. Exhaustive on purpose: a new variant must make a
/// conscious display choice instead of silently inheriting a JSON dump.
fn tool_detail_lines(details: &ThreadItemDetails, workspace_root: Option<&Path>) -> Vec<String> {
    match details {
        ThreadItemDetails::ToolInvocation(tool) => invocation_lines(tool, workspace_root),
        ThreadItemDetails::McpToolCall(call) => call
            .arguments
            .as_ref()
            .map(|args| collect_param_details(&call.tool_name, args, &[], workspace_root))
            .unwrap_or_default(),
        // The harness message already sits on the status line and the command/path on
        // the title; only a failing exit code adds information.
        ThreadItemDetails::Harness(event) => event
            .exit_code
            .filter(|code| *code != 0)
            .map(|code| format!("Exit {code}"))
            .into_iter()
            .collect(),
        // Outputs live in `activity_output_text`, and searches/errors carry their
        // content in the title or status line already. Message-like variants never
        // reach this path; they project to assistant rows earlier. (Run *invocations*
        // go through `invocation_lines`, where the title-consumed command keys usually
        // leave nothing; only unusual extra parameters render.)
        ThreadItemDetails::CommandExecution(_)
        | ThreadItemDetails::ToolOutput(_)
        | ThreadItemDetails::WebSearch(_)
        | ThreadItemDetails::Error(_)
        | ThreadItemDetails::AgentMessage(_)
        | ThreadItemDetails::Plan(_)
        | ThreadItemDetails::Reasoning(_)
        | ThreadItemDetails::Decision(_)
        | ThreadItemDetails::FileChange(_) => Vec::new(),
    }
}

fn invocation_lines(tool: &ToolInvocationItem, workspace_root: Option<&Path>) -> Vec<String> {
    let args = match tool.arguments.as_ref() {
        Some(args) => args,
        None => return Vec::new(),
    };
    // A session title doubles as the session predicate (shared with the row title so
    // the two can never disagree): titled follow-ups collapse to the shared
    // one-row identity, while launches keep generic details.
    if session_tool_title(&tool.tool_name, Some(args)).is_some() {
        return exec_session_param_detail(args).into_iter().collect();
    }
    let category = ToolCategory::from_tool_name(&tool.tool_name, Some(args));
    // Both key sets are static; merging keeps the shared collector's single-slice shape.
    let skipped: Vec<&str> = gist_keys(category)
        .iter()
        .chain(extra_gist_keys(tool).iter())
        .copied()
        .collect();
    collect_param_details(&tool.tool_name, args, &skipped, workspace_root)
}

/// Argument keys the row title already shows for a category, so details skip them.
///
/// Contract with the gist readers in `activity_verbs.rs` (`file_basename`,
/// `command_text`, fetch URL): a key added to a category-wide gist must join
/// its set here, or the title and body repeat each other. Search consumption
/// is arg-dependent (the title phrases the matched alias only), so every
/// search key reports via `extra_gist_keys` from the same reader as the
/// title — a category-wide set would over-hide siblings the title never
/// named. `gist_keys_stay_silent_in_details` pins the agreement behaviorally.
fn gist_keys(category: ToolCategory) -> &'static [&'static str] {
    match category {
        ToolCategory::Read => &["path", "file_path", "filepath", "target_path", "url"],
        ToolCategory::Edit => &["path", "file_path", "filepath", "target_path"],
        ToolCategory::Run => &["command", "cmd", "raw_command", "bash_command"],
        // Search keys ride `extra_gist_keys`: consumption varies per call.
        ToolCategory::Search => &[],
        ToolCategory::Tracker | ToolCategory::Other => &[],
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use vtcode_config::constants::tools as tool_names;
    use vtcode_exec_events::{
        CommandExecutionItem, CommandExecutionStatus, HarnessEventItem, HarnessEventKind, McpToolCallItem,
        ThreadItemDetails, ToolCallStatus, ToolInvocationItem, ToolOutputItem, WebSearchItem,
    };

    use super::*;

    fn invocation(name: &str, arguments: Option<Value>) -> ThreadItemDetails {
        ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: name.into(),
            arguments,
            tool_call_id: None,
            status: ToolCallStatus::Completed,
            outcome: None,
        }))
    }

    fn summary(details: &ThreadItemDetails) -> String {
        activity_details_summary(details, None)
    }

    #[test]
    fn summary_skips_gist_noise_and_verbose_keys() {
        // Path feeds the title gist, the rest is plumbing or verbose content: nothing
        // remains for the body. The asymmetric twin proves one real knob survives.
        let details = invocation(
            "read_file",
            Some(json!({"path": "src/main.rs", "timeout_secs": 30, "action": "read", "max_bytes": 4096})),
        );
        assert_eq!(summary(&details), "");
        let with_knob = invocation("read_file", Some(json!({"path": "src/main.rs", "limit": 50})));
        assert_eq!(summary(&with_knob), "Limit: 50");
    }

    #[test]
    fn summary_shows_human_lines_for_unconsumed_args() {
        let details = invocation("task_tracker", Some(json!({"limit": 25, "verbose": true, "tags": ["a", "b", "c"]})));
        let mut lines = summary(&details).lines().map(str::to_string).collect::<Vec<_>>();
        lines.sort();
        assert_eq!(lines, ["Limit: 25", "Tags: a, b +1 more", "Verbose"]);
    }

    #[test]
    fn summary_omits_zero_false_blank_and_structured_values() {
        // Both sides of each boundary: zero/false/blank/null/object vanish …
        let details = invocation(
            "task_tracker",
            Some(json!({"limit": 0, "verbose": false, "note": "", "nothing": null, "opts": {"a": 1}})),
        );
        assert_eq!(summary(&details), "");
        // … while nonzero/true/non-blank render.
        let details = invocation("task_tracker", Some(json!({"limit": 1, "verbose": true, "note": "hi"})));
        let mut lines = summary(&details).lines().map(str::to_string).collect::<Vec<_>>();
        lines.sort();
        assert_eq!(lines, ["Limit: 1", "Note: hi", "Verbose"]);
    }

    #[test]
    fn summary_truncates_long_values() {
        let details = invocation("task_tracker", Some(json!({"note": "x".repeat(100)})));
        let line = summary(&details);
        assert!(line.starts_with("Note: "), "keeps the human key: {line}");
        assert!(line.contains('…'), "long values ellipsize: {line}");
        assert!(line.len() < 100, "bounded line: {line}");
    }

    #[test]
    fn summary_relativizes_paths_against_workspace() {
        let details = invocation("task_tracker", Some(json!({"destination": "/ws/src/a.rs", "limit": 1})));
        let root = Path::new("/ws");
        let mut lines = activity_details_summary(&details, Some(root))
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        lines.sort();
        assert_eq!(lines, ["Destination: src/a.rs", "Limit: 1"]);
        // Asymmetric: without a workspace the absolute path survives verbatim.
        assert!(summary(&details).contains("/ws/src/a.rs"));
    }

    #[test]
    fn session_followups_collapse_to_identity_row() {
        // The session id plus the wait deadline survive; token caps and the raw
        // stdin payload do not.
        let details = invocation(
            tool_names::WRITE_STDIN,
            Some(json!({"session_id": "run-1", "wait_timeout_seconds": 600, "max_output_tokens": 4000, "input": "ls"})),
        );
        assert_eq!(summary(&details), "Session run-1 · wait up to 600s");
        // Asymmetric: identity alone, deadline alone, and neither.
        let id_only = invocation(tool_names::WRITE_STDIN, Some(json!({"session_id": "run-1"})));
        assert_eq!(summary(&id_only), "Session run-1");
        let wait_only = invocation(tool_names::WRITE_STDIN, Some(json!({"timeout_seconds": 30})));
        assert_eq!(summary(&wait_only), "wait up to 30s");
        let neither = invocation(tool_names::WRITE_STDIN, Some(json!({})));
        assert_eq!(summary(&neither), "");
        // A zero deadline is not a deadline.
        let zero_wait =
            invocation(tool_names::WRITE_STDIN, Some(json!({"session_id": "run-1", "wait_timeout_seconds": 0})));
        assert_eq!(summary(&zero_wait), "Session run-1");
    }

    #[test]
    fn session_detection_follows_canonical_tool_names() {
        // Driven by the canonical registry constants: if desktop literals drift,
        // these titles (and their detail rows) stop matching.
        for name in [tool_names::SEND_PTY_INPUT, tool_names::READ_PTY_SESSION] {
            let details = invocation(name, Some(json!({"session_id": "s-1"})));
            assert_eq!(summary(&details), "Session s-1", "{name} is a session follow-up");
        }
        let poll = invocation(tool_names::UNIFIED_EXEC, Some(json!({"action": "poll", "session_id": "s-2"})));
        assert_eq!(summary(&poll), "Session s-2");
        // Asymmetric: a launch keeps generic (here empty) details, not a session row.
        let run = invocation(tool_names::UNIFIED_EXEC, Some(json!({"action": "run", "cmd": "ls"})));
        assert_eq!(summary(&run), "");
    }

    #[test]
    fn gist_keys_stay_silent_in_details() {
        // Pins the contract with the title gist: keys the title shows must not echo.
        // If a gist reader gains a key, `gist_keys` must gain it too.
        for (name, args) in [
            ("read_file", json!({"path": "a.rs"})),
            ("edit_file", json!({"file_path": "b.rs"})),
            ("exec_command", json!({"command": "cargo check"})),
            ("code_search", json!({"query": "Widget"})),
        ] {
            assert_eq!(summary(&invocation(name, Some(args))), "", "{name} gist keys stay silent");
        }
        assert_eq!(summary(&invocation("read_file", None)), "");
    }

    #[test]
    fn tool_specific_headline_keys_stay_silent_in_details() {
        // The title already phrases these: grep_file's pattern and scope,
        // list_files' location and patterns. An unconsumed knob on the same
        // call still renders, proving the silence is keyed.
        assert_eq!(summary(&invocation("grep_file", Some(json!({"pattern": "TODO", "path": "src"})))), "");
        assert_eq!(summary(&invocation("list_files", Some(json!({"name_pattern": "*.rs"})))), "");
        assert_eq!(summary(&invocation("list_files", Some(json!({"content_pattern": "TODO"})))), "");
        let with_knob = invocation("grep_file", Some(json!({"pattern": "TODO", "path": "src", "limit": 5})));
        assert_eq!(summary(&with_knob), "Limit: 5");
        // Asymmetric: a plain grep keeps its scope in the details, since only
        // grep_file phrases it in the title.
        assert_eq!(summary(&invocation("grep", Some(json!({"pattern": "TODO", "path": "src"})))), "Path: src");
        // Silence follows consumption, not the tool: skipped keys show, and
        // headline-less calls hide nothing.
        assert_eq!(
            summary(&invocation("list_files", Some(json!({"path": "src", "name_pattern": "*.rs"})))),
            "Name pattern: *.rs"
        );
        assert_eq!(summary(&invocation("grep_file", Some(json!({"path": "src"})))), "Path: src");
        assert_eq!(summary(&invocation("code_search", Some(json!({"q": "Widget"})))), "");
        // code_search silence follows the matched alias, not the category: an
        // unphrased sibling still renders.
        assert_eq!(summary(&invocation("code_search", Some(json!({"query": "A", "pattern": "B"})))), "Pattern: B");
        assert_eq!(summary(&invocation("code_search", Some(json!({"pattern": "A", "q": "B"})))), "Q: B");
        // Search aliases hide only when the headline consumed them: an
        // unphrased sibling shows instead of vanishing as noise/verbose.
        assert_eq!(summary(&invocation("grep", Some(json!({"pattern": "A", "query": "B"})))), "Query: B");
        assert_eq!(summary(&invocation("code_search", Some(json!({"pattern": "A", "text": "T"})))), "Text: T");
        // Asymmetric: `text` stays a hidden body for edits.
        assert_eq!(summary(&invocation("edit_file", Some(json!({"path": "f", "text": "body"})))), "");
    }

    #[test]
    fn output_text_routes_content_per_variant() {
        let command = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check".into(),
            arguments: None,
            aggregated_output: "output body".into(),
            exit_code: None,
            status: CommandExecutionStatus::Completed,
        }));
        assert_eq!(activity_output_text(&command).as_deref(), Some("output body"));
        let tool_output = ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: "call-1".into(),
            tool_call_id: None,
            spool_path: None,
            output: "tool body".into(),
            is_delta: None,
            exit_code: None,
            status: ToolCallStatus::Completed,
            app_previews: Vec::new(),
        }));
        assert_eq!(activity_output_text(&tool_output).as_deref(), Some("tool body"));
        // MCP results and search hits join the outputs so no content is lost with
        // the JSON dump; invocations carry none.
        let mcp = ThreadItemDetails::McpToolCall(Box::new(McpToolCallItem {
            tool_name: "review".into(),
            arguments: None,
            result: Some("review body".into()),
            status: None,
        }));
        assert_eq!(activity_output_text(&mcp).as_deref(), Some("review body"));
        let searches = ThreadItemDetails::WebSearch(Box::new(WebSearchItem {
            query: "q".into(),
            provider: None,
            results: Some(vec!["first".into(), "second".into()]),
        }));
        assert_eq!(activity_output_text(&searches).as_deref(), Some("first\n\nsecond"));
        assert_eq!(activity_output_text(&invocation("read_file", None)), None);
    }

    #[test]
    fn output_text_ignores_absent_or_empty_results() {
        let no_results =
            ThreadItemDetails::WebSearch(Box::new(WebSearchItem { query: "q".into(), provider: None, results: None }));
        assert_eq!(activity_output_text(&no_results), None);
        let empty_results = ThreadItemDetails::WebSearch(Box::new(WebSearchItem {
            query: "q".into(),
            provider: None,
            results: Some(Vec::new()),
        }));
        assert_eq!(activity_output_text(&empty_results), None);
        let no_result = ThreadItemDetails::McpToolCall(Box::new(McpToolCallItem {
            tool_name: "review".into(),
            arguments: None,
            result: None,
            status: None,
        }));
        assert_eq!(activity_output_text(&no_result), None);
    }

    fn command_output(aggregated: &str) -> ThreadItemDetails {
        ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check".into(),
            arguments: None,
            aggregated_output: aggregated.into(),
            exit_code: None,
            status: CommandExecutionStatus::Completed,
        }))
    }

    #[test]
    fn output_text_strips_structured_envelope_but_keeps_spool_note() {
        let details = command_output(
            "log line\nStructured output:\n{\"backend\": \"pipe\", \"spool_note\": \"Spooled to out.txt\", \"pid\": 1}",
        );
        assert_eq!(activity_output_text(&details).as_deref(), Some("log line\nSpooled to out.txt"));
        // Asymmetric: no note anywhere — only the prose survives.
        let details = command_output("log line\nStructured output:\n{\"backend\": \"pipe\"}");
        assert_eq!(activity_output_text(&details).as_deref(), Some("log line"));
    }

    #[test]
    fn output_text_collapses_blank_captures_to_none() {
        for blank in ["", "   ", "\n\n"] {
            assert_eq!(activity_output_text(&command_output(blank)), None, "{blank:?} collapses");
        }
        // A metadata-only block with no note is blank after the strip.
        let details = command_output("Structured output:\n{\"backend\": \"pipe\"}");
        assert_eq!(activity_output_text(&details), None);
        // Asymmetric: the same block with a note stays `Some`.
        let details = command_output("Structured output:\n{\"spool_note\": \"Spooled\"}");
        assert_eq!(activity_output_text(&details).as_deref(), Some("Spooled"));
    }

    #[test]
    fn output_text_never_carries_structured_envelope() {
        let variants = [
            command_output("body\nStructured output:\n{\"backend\": \"pipe\", \"pid\": 1}"),
            ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "call-1".into(),
                tool_call_id: None,
                spool_path: None,
                output: "body\nStructured output:\n{\"lifecycle_state\": \"exited\"}".into(),
                is_delta: None,
                exit_code: None,
                status: ToolCallStatus::Completed,
                app_previews: Vec::new(),
            })),
        ];
        for details in &variants {
            let text = activity_output_text(details).expect("prose survives the strip");
            for marker in ["Structured output:", "\"backend\"", "\"pid\"", "\"lifecycle_state\""] {
                assert!(!text.contains(marker), "no envelope marker {marker:?} in {text:?}");
            }
        }
        // Asymmetric: MCP results never carry core's envelope, so even
        // envelope-shaped server content passes through verbatim — the strip
        // must not eat genuine result bytes.
        let mcp = ThreadItemDetails::McpToolCall(Box::new(McpToolCallItem {
            tool_name: "review".into(),
            arguments: None,
            result: Some("body\nStructured output:\n{\"total_output_bytes\": 9}".into()),
            status: None,
        }));
        assert_eq!(
            activity_output_text(&mcp).as_deref(),
            Some("body\nStructured output:\n{\"total_output_bytes\": 9}")
        );
    }

    #[test]
    fn harness_reports_only_failing_exit_codes() {
        let harness = |exit_code| {
            let mut item = HarnessEventItem::new(HarnessEventKind::VerificationFailed);
            item.message = Some("verified".into());
            item.exit_code = exit_code;
            ThreadItemDetails::Harness(Box::new(item))
        };
        assert_eq!(summary(&harness(Some(3))), "Exit 3");
        assert_eq!(summary(&harness(Some(0))), "");
        assert_eq!(summary(&harness(None)), "");
    }

    #[test]
    fn summary_never_carries_json_markers() {
        let variants = [
            invocation("read_file", Some(json!({"path": "a.rs", "limit": 3}))),
            invocation(tool_names::WRITE_STDIN, Some(json!({"session_id": "s"}))),
            ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "call-1".into(),
                tool_call_id: Some("provider-1".into()),
                spool_path: Some("spool".into()),
                output: "body".into(),
                is_delta: None,
                exit_code: Some(0),
                status: ToolCallStatus::Completed,
                app_previews: Vec::new(),
            })),
            ThreadItemDetails::McpToolCall(Box::new(McpToolCallItem {
                tool_name: "review".into(),
                arguments: Some(json!({"target": "main.rs"})),
                result: Some("body".into()),
                status: None,
            })),
            ThreadItemDetails::WebSearch(Box::new(WebSearchItem {
                query: "q".into(),
                provider: Some("p".into()),
                results: Some(vec!["hit".into()]),
            })),
        ];
        for details in &variants {
            let text = summary(details);
            for marker in ["\"tool_name\"", "\"type\"", "{\n", "call-1", "provider-1"] {
                assert!(!text.contains(marker), "no dump marker {marker:?} in {text:?}");
            }
        }
    }

    #[test]
    fn activity_text_collapses_empty_payload() {
        assert_eq!(activity_text("completed", ""), "completed");
        assert_eq!(activity_text("completed", "A\nB"), "completed\n\nA\nB");
    }

    #[test]
    fn activity_payload_tolerates_shapes() {
        assert_eq!(activity_payload("completed\n\nA"), "A");
        assert_eq!(activity_payload("completed"), "");
        assert_eq!(activity_payload("completed\nA"), "A");
    }
}
