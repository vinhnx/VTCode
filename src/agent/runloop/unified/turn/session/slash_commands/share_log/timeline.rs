//! Agent Legibility:
//! - Entrypoint: `build_timeline_export` builds the exported timeline model and `render_session_timeline_html` renders the self-contained HTML artifact.
//! - Common changes:
//!   - Session overview aggregation stays in this root.
//!   - Timeline row shaping, redaction, and HTML presentation live in `timeline/presentation.rs`.
//! - Constraints: Keep thread-event exports preferred over conversation fallback exports, and preserve redaction-on-export behavior.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode timeline`

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use vtcode_core::core::threads::ThreadEventRecord;
use vtcode_core::exec::events::{PatchChangeKind, ThreadEvent, ThreadItemDetails};
use vtcode_core::llm::provider as uni;

#[path = "timeline/presentation.rs"]
mod presentation;

#[cfg(test)]
use presentation::timeline_row_from_item;
use presentation::{timeline_rows_from_messages, timeline_rows_from_thread_events};

const TIMELINE_SOURCE_THREAD_EVENTS: &str = "thread_events";
const TIMELINE_SOURCE_CONVERSATION_FALLBACK: &str = "conversation_fallback";
const SUMMARY_PREVIEW_LIMIT: usize = 120;
const REDACTION_NOTICE: &str = "Sensitive values are redacted by default in exported logs.";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct TimelineExport {
    exported_at: String,
    model: String,
    provider: String,
    workspace: String,
    thread_id: String,
    source: String,
    total_rows: usize,
    redaction_enabled: bool,
    overview: SessionOverview,
    rows: Vec<TimelineRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct SessionOverview {
    provider: String,
    model: String,
    api_calls: u64,
    turns: u64,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cache_creation_tokens: u64,
    total_tokens: u64,
    added_files: u64,
    updated_files: u64,
    deleted_files: u64,
    total_file_changes: u64,
    prompt_cache_observations: usize,
    prompt_cache_model_changes: usize,
    prompt_cache_unchanged: usize,
    prompt_cache_stable_prefix_changes: usize,
    prompt_cache_tool_catalog_changes: usize,
    prompt_cache_combined_changes: usize,
    last_prompt_cache_change_reason: Option<String>,
    source: String,
    total_rows: usize,
    outcome_code: Option<String>,
    total_cost_usd: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct TimelineRow {
    sequence: u64,
    source: String,
    event_type: String,
    item_type: Option<String>,
    category: String,
    role: String,
    transcript_kind: String,
    status: Option<String>,
    turn_id: Option<String>,
    submission_id: Option<String>,
    title: String,
    summary: String,
    body: String,
    detail_json: Option<String>,
    is_low_signal: bool,
}

pub(super) fn build_timeline_export(
    exported_at: &str,
    provider: &str,
    model: &str,
    workspace: &std::path::Path,
    thread_id: &str,
    event_records: &[ThreadEventRecord],
    messages: &[uni::Message],
    prompt_cache_diagnostics: Option<
        &crate::agent::runloop::unified::state::PromptCacheDiagnostics,
    >,
) -> TimelineExport {
    let (source, rows) = if event_records.is_empty() {
        (
            TIMELINE_SOURCE_CONVERSATION_FALLBACK.to_string(),
            timeline_rows_from_messages(&super::build_session_log_messages(messages)),
        )
    } else {
        (
            TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
            timeline_rows_from_thread_events(event_records),
        )
    };
    let overview = build_session_overview(
        provider,
        model,
        &source,
        rows.len(),
        event_records,
        prompt_cache_diagnostics,
    );

    TimelineExport {
        exported_at: exported_at.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        workspace: workspace.display().to_string(),
        thread_id: thread_id.to_string(),
        source,
        total_rows: rows.len(),
        redaction_enabled: true,
        overview,
        rows,
    }
}

pub(super) fn render_session_timeline_html(
    export: &TimelineExport,
    session_log_json: &Value,
) -> Result<String> {
    presentation::render_session_timeline_html(export, session_log_json)
}

pub(super) fn redact_timeline_export(export: &TimelineExport) -> TimelineExport {
    presentation::redact_timeline_export(export)
}

fn build_session_overview(
    provider: &str,
    model: &str,
    source: &str,
    total_rows: usize,
    event_records: &[ThreadEventRecord],
    prompt_cache_diagnostics: Option<
        &crate::agent::runloop::unified::state::PromptCacheDiagnostics,
    >,
) -> SessionOverview {
    let mut api_calls = 0_u64;
    let mut turns = 0_u64;
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut cached_input_tokens = 0_u64;
    let mut cache_creation_tokens = 0_u64;
    let mut added_files = 0_u64;
    let mut updated_files = 0_u64;
    let mut deleted_files = 0_u64;
    let mut outcome_code = None;
    let mut total_cost_usd = None;

    for record in event_records {
        match &record.event {
            ThreadEvent::TurnCompleted(event) => {
                api_calls += 1;
                turns += 1;
                input_tokens += event.usage.input_tokens;
                output_tokens += event.usage.output_tokens;
                cached_input_tokens += event.usage.cached_input_tokens;
                cache_creation_tokens += event.usage.cache_creation_tokens;
            }
            ThreadEvent::ThreadCompleted(event) => {
                if turns == 0 {
                    turns = u64::try_from(event.num_turns).unwrap_or(u64::MAX);
                }
                outcome_code = Some(event.outcome_code.clone());
                total_cost_usd = event.total_cost_usd.as_ref().map(ToString::to_string);
            }
            ThreadEvent::ItemCompleted(event) => {
                if let ThreadItemDetails::FileChange(file_change) = &event.item.details {
                    for change in &file_change.changes {
                        match change.kind {
                            PatchChangeKind::Add => added_files += 1,
                            PatchChangeKind::Update => updated_files += 1,
                            PatchChangeKind::Delete => deleted_files += 1,
                        }
                    }
                }
            }
            ThreadEvent::ItemUpdated(event) => {
                if let ThreadItemDetails::FileChange(file_change) = &event.item.details {
                    for change in &file_change.changes {
                        match change.kind {
                            PatchChangeKind::Add => added_files += 1,
                            PatchChangeKind::Update => updated_files += 1,
                            PatchChangeKind::Delete => deleted_files += 1,
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let prompt_cache_diagnostics = prompt_cache_diagnostics.cloned().unwrap_or_default();

    SessionOverview {
        provider: provider.to_string(),
        model: model.to_string(),
        api_calls,
        turns,
        input_tokens,
        output_tokens,
        cached_input_tokens,
        cache_creation_tokens,
        total_tokens: input_tokens.saturating_add(output_tokens),
        added_files,
        updated_files,
        deleted_files,
        total_file_changes: added_files + updated_files + deleted_files,
        prompt_cache_observations: prompt_cache_diagnostics.observations,
        prompt_cache_model_changes: prompt_cache_diagnostics.model_changes,
        prompt_cache_unchanged: prompt_cache_diagnostics.unchanged,
        prompt_cache_stable_prefix_changes: prompt_cache_diagnostics.stable_prefix_changes,
        prompt_cache_tool_catalog_changes: prompt_cache_diagnostics.tool_catalog_changes,
        prompt_cache_combined_changes: prompt_cache_diagnostics.combined_changes,
        last_prompt_cache_change_reason: prompt_cache_diagnostics.last_change_reason,
        source: source.to_string(),
        total_rows,
        outcome_code,
        total_cost_usd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::core::threads::{ThreadEventRecord, ThreadId};
    use vtcode_core::exec::events::{
        AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ItemCompletedEvent,
        ItemStartedEvent, ThreadItem, ThreadStartedEvent, ToolCallStatus, ToolInvocationItem,
        TurnCompletedEvent, Usage,
    };

    fn sample_event_record(sequence: u64, event: ThreadEvent) -> ThreadEventRecord {
        ThreadEventRecord {
            sequence,
            thread_id: ThreadId::new("thread-1"),
            submission_id: None,
            turn_id: Some("turn-1".to_string()),
            event,
        }
    }

    #[test]
    fn timeline_export_prefers_thread_events() {
        let records = vec![
            sample_event_record(
                1,
                ThreadEvent::ThreadStarted(ThreadStartedEvent {
                    thread_id: "thread-1".to_string(),
                }),
            ),
            sample_event_record(
                2,
                ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem {
                        id: "msg-1".to_string(),
                        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                            text: "assistant reply".to_string(),
                        }),
                    },
                }),
            ),
        ];
        let messages = vec![uni::Message {
            role: uni::MessageRole::User,
            content: uni::MessageContent::Text("hello".to_string()),
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            phase: None,
            origin_tool: None,
        }];

        let export = build_timeline_export(
            "2026-04-04T00:00:00Z",
            "openai",
            "gpt-test",
            std::path::Path::new("/tmp/workspace"),
            "thread-1",
            &records,
            &messages,
            None,
        );

        assert_eq!(export.source, TIMELINE_SOURCE_THREAD_EVENTS);
        assert_eq!(export.rows.len(), 2);
        assert_eq!(export.rows[1].category, "message");
    }

    #[test]
    fn timeline_export_falls_back_to_conversation_messages() {
        let messages = vec![json!({
            "role": "Assistant",
            "content": "assistant output",
            "tool_calls": [{
                "id": "call-1",
                "function": {
                    "name": "exec_command",
                    "arguments": {"cmd": "pwd"}
                }
            }]
        })];

        let rows = timeline_rows_from_messages(&messages);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].source, TIMELINE_SOURCE_CONVERSATION_FALLBACK);
        assert_eq!(rows[0].category, "message");
        assert_eq!(rows[1].category, "tool");
    }

    #[test]
    fn html_timeline_is_self_contained_and_escapes_embedded_json() {
        let records = vec![
            sample_event_record(
                1,
                ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem {
                        id: "msg-1".to_string(),
                        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                            text: "<script>alert('xss')</script>".to_string(),
                        }),
                    },
                }),
            ),
            sample_event_record(
                2,
                ThreadEvent::ItemStarted(ItemStartedEvent {
                    item: ThreadItem {
                        id: "tool-1".to_string(),
                        details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                            tool_name: "exec_command".to_string(),
                            arguments: Some(json!({"cmd": "pwd"})),
                            tool_call_id: Some("call-1".to_string()),
                            status: ToolCallStatus::InProgress,
                        }),
                    },
                }),
            ),
            sample_event_record(
                3,
                ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    usage: Usage {
                        input_tokens: 10,
                        cached_input_tokens: 2,
                        cache_creation_tokens: 0,
                        output_tokens: 4,
                    },
                }),
            ),
        ];

        let export = build_timeline_export(
            "2026-04-04T00:00:00Z",
            "openai",
            "gpt-test",
            std::path::Path::new("/tmp/workspace"),
            "thread-1",
            &records,
            &[],
            None,
        );
        let html = render_session_timeline_html(&export, &json!({"messages": []})).expect("html");

        assert!(html.contains("id=\"search-input\""));
        assert!(html.contains("id=\"category-filter\""));
        assert!(html.contains("id=\"status-filter\""));
        assert!(html.contains("id=\"hide-low-signal\""));
        assert!(html.contains("vtcode-session-data"));
        assert!(html.contains("vtcode-session-log-data"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.contains("<script>alert('xss')</script>"));
        assert!(html.contains("\\u003cscript\\u003ealert('xss')\\u003c/script\\u003e"));
    }

    #[test]
    fn html_timeline_uses_flat_surfaces_without_shadows() {
        let export = TimelineExport {
            exported_at: "2026-04-04T00:00:00Z".to_string(),
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            workspace: "/tmp/workspace".to_string(),
            thread_id: "thread-1".to_string(),
            source: TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
            total_rows: 0,
            redaction_enabled: true,
            overview: SessionOverview {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_calls: 1,
                turns: 1,
                input_tokens: 10,
                output_tokens: 4,
                cached_input_tokens: 2,
                cache_creation_tokens: 0,
                total_tokens: 14,
                added_files: 1,
                updated_files: 2,
                deleted_files: 0,
                total_file_changes: 3,
                prompt_cache_observations: 0,
                prompt_cache_model_changes: 0,
                prompt_cache_unchanged: 0,
                prompt_cache_stable_prefix_changes: 0,
                prompt_cache_tool_catalog_changes: 0,
                prompt_cache_combined_changes: 0,
                last_prompt_cache_change_reason: None,
                source: TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
                total_rows: 0,
                outcome_code: Some("completed".to_string()),
                total_cost_usd: None,
            },
            rows: Vec::new(),
        };

        let html = render_session_timeline_html(&export, &json!({"messages": []})).expect("html");

        assert!(!html.contains("--shadow"));
        assert!(!html.contains("box-shadow"));
        assert!(!html.contains("border-top:4px solid var(--accent)"));
        assert!(html.contains("Session Overview"));
        assert!(html.contains("Shared Thread"));
        assert!(html.contains("VT Code Thread Share"));
        assert!(html.contains("Search &amp; Filters"));
        assert!(html.contains("Open redacted JSON log"));
    }

    #[test]
    fn html_timeline_surfaces_prompt_cache_overview_fields() {
        let export = TimelineExport {
            exported_at: "2026-04-04T00:00:00Z".to_string(),
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            workspace: "/tmp/workspace".to_string(),
            thread_id: "thread-1".to_string(),
            source: TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
            total_rows: 0,
            redaction_enabled: true,
            overview: SessionOverview {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_calls: 2,
                turns: 2,
                input_tokens: 100,
                output_tokens: 40,
                cached_input_tokens: 20,
                cache_creation_tokens: 10,
                total_tokens: 140,
                added_files: 1,
                updated_files: 0,
                deleted_files: 0,
                total_file_changes: 1,
                prompt_cache_observations: 5,
                prompt_cache_model_changes: 1,
                prompt_cache_unchanged: 1,
                prompt_cache_stable_prefix_changes: 2,
                prompt_cache_tool_catalog_changes: 0,
                prompt_cache_combined_changes: 1,
                last_prompt_cache_change_reason: Some("stable_prefix".to_string()),
                source: TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
                total_rows: 0,
                outcome_code: Some("completed".to_string()),
                total_cost_usd: None,
            },
            rows: Vec::new(),
        };

        let html = render_session_timeline_html(&export, &json!({"messages": []})).expect("html");

        assert!(html.contains("Prompt cache observations"));
        assert!(html.contains("Cache churn"));
        assert!(html.contains("Last cache change"));
        assert!(html.contains("stable_prefix"));
        assert!(html.contains("tool_catalog"));
    }

    #[test]
    fn html_timeline_avoids_duplicate_summary_rows_for_fallback_messages() {
        let export = TimelineExport {
            exported_at: "2026-04-04T00:00:00Z".to_string(),
            provider: "copilot".to_string(),
            model: "claude-haiku-4.5".to_string(),
            workspace: "/tmp/workspace".to_string(),
            thread_id: "thread-1".to_string(),
            source: TIMELINE_SOURCE_CONVERSATION_FALLBACK.to_string(),
            total_rows: 1,
            redaction_enabled: true,
            overview: SessionOverview {
                provider: "copilot".to_string(),
                model: "claude-haiku-4.5".to_string(),
                api_calls: 0,
                turns: 0,
                input_tokens: 0,
                output_tokens: 0,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
                total_tokens: 0,
                added_files: 0,
                updated_files: 0,
                deleted_files: 0,
                total_file_changes: 0,
                prompt_cache_observations: 0,
                prompt_cache_model_changes: 0,
                prompt_cache_unchanged: 0,
                prompt_cache_stable_prefix_changes: 0,
                prompt_cache_tool_catalog_changes: 0,
                prompt_cache_combined_changes: 0,
                last_prompt_cache_change_reason: None,
                source: TIMELINE_SOURCE_CONVERSATION_FALLBACK.to_string(),
                total_rows: 1,
                outcome_code: None,
                total_cost_usd: None,
            },
            rows: vec![TimelineRow {
                sequence: 1,
                source: TIMELINE_SOURCE_CONVERSATION_FALLBACK.to_string(),
                event_type: "conversation.message".to_string(),
                item_type: Some("message".to_string()),
                category: "message".to_string(),
                role: "assistant".to_string(),
                transcript_kind: "message".to_string(),
                status: Some("completed".to_string()),
                turn_id: None,
                submission_id: None,
                title: "Assistant message".to_string(),
                summary: "This is VT Code.".to_string(),
                body: "This is VT Code.\n\nKey features:\n- Safe shell execution\n- Thread timeline exports".to_string(),
                detail_json: Some("{\"role\":\"Assistant\"}".to_string()),
                is_low_signal: false,
            }],
        };

        let html = render_session_timeline_html(&export, &json!({"messages": []})).expect("html");

        assert!(html.contains("messageBody(row)"));
        assert!(html.contains("renderMessageBlocks"));
        assert!(html.contains("normalizeText(body) === normalizeText(summary)"));
        assert!(html.contains("Key features"));
        assert!(html.contains("Safe shell execution"));
        assert!(html.contains("Thread timeline exports"));
    }

    #[test]
    fn command_rows_surface_status_and_output() {
        let row = timeline_row_from_item(
            &sample_event_record(
                7,
                ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem {
                        id: "cmd-1".to_string(),
                        details: ThreadItemDetails::CommandExecution(Box::new(
                            CommandExecutionItem {
                                command: "cargo check".to_string(),
                                arguments: Some(json!({"args": ["-p", "vtcode"]})),
                                aggregated_output: "Finished dev [unoptimized]".to_string(),
                                exit_code: Some(0),
                                status: CommandExecutionStatus::Completed,
                            },
                        )),
                    },
                }),
            ),
            "item.completed",
            "completed",
            &ThreadItem {
                id: "cmd-1".to_string(),
                details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: "cargo check".to_string(),
                    arguments: Some(json!({"args": ["-p", "vtcode"]})),
                    aggregated_output: "Finished dev [unoptimized]".to_string(),
                    exit_code: Some(0),
                    status: CommandExecutionStatus::Completed,
                })),
            },
        );

        assert_eq!(row.category, "command");
        assert_eq!(row.status.as_deref(), Some("completed"));
        assert!(row.summary.contains("exit 0"));
        assert!(row.body.contains("Finished dev"));
    }
}
