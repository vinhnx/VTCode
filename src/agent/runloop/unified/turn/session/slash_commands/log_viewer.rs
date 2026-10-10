use std::io::BufRead;
use std::path::PathBuf;

use anyhow::{Context, Result};
use vtcode_commons::sanitizer::redact_secrets;
use vtcode_core::core::threads::ThreadEventRecord;
use vtcode_core::exec::events::VersionedThreadEvent;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::file_utils::write_file_with_context_sync;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::{LogFormat, LogScope};

const MAX_EVENTS_IN_MODAL: usize = 2000;
const MAX_FIELD_DISPLAY_LEN: usize = 200;

pub(crate) async fn handle_show_log_viewer(
    ctx: SlashCommandContext<'_>,
    format: LogFormat,
    scope: LogScope,
    save: bool,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(MessageStyle::Error, "Log viewer requires inline UI mode.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let entries = match scope {
        LogScope::Thread => read_from_memory(ctx.thread_handle)?,
        LogScope::All => {
            // The memory read is non-blocking; the disk scan does
            // `std::fs::read_dir` + per-file reads, so run it off the async
            // executor. See `# Blocking` docs in `src/agent/runloop/git.rs`.
            let mut all_entries = read_from_memory(ctx.thread_handle)?;
            let workspace = ctx.config.workspace.clone();
            let disk_entries = tokio::task::spawn_blocking(move || collect_disk_entries(&workspace))
                .await
                .context("log disk scan task panicked")?;
            all_entries.extend(disk_entries);
            // Sort by seq for approximate chronological ordering. We do NOT
            // dedup: memory entries use the global `record.sequence` while disk
            // entries use per-file line indices (see `read_events_from_file`),
            // so a `dedup_by_key(|e| e.seq)` would incorrectly drop events from
            // different sessions that happen to share a line number.
            all_entries.sort_by_key(|e| e.seq);
            all_entries
        }
    };

    if entries.is_empty() {
        ctx.renderer.line(MessageStyle::Info, "No events recorded yet.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let formatted = match format {
        LogFormat::Text => format_entries_text(&entries),
        LogFormat::Json => format_entries_json(&entries),
    };

    if save {
        let path = save_log_file(&ctx.config.workspace, &formatted, format)?;
        ctx.renderer
            .line(MessageStyle::Info, &format!("Session log saved to: {}", path.display()))?;
        return Ok(SlashCommandControl::Continue);
    }

    let display_lines = if entries.len() > MAX_EVENTS_IN_MODAL {
        let truncated = &entries[..MAX_EVENTS_IN_MODAL];
        match format {
            LogFormat::Text => {
                let mut lines = format_entries_text(truncated);
                lines.push_str(&format!(
                    "\n... ({} of {} events shown, use --save to export full log)",
                    MAX_EVENTS_IN_MODAL,
                    entries.len()
                ));
                lines
            }
            LogFormat::Json => {
                let mut lines = format_entries_json(truncated);
                lines.push_str(&format!(
                    "\n// ... ({} of {} events shown, use --save to export full log)",
                    MAX_EVENTS_IN_MODAL,
                    entries.len()
                ));
                lines
            }
        }
    } else {
        match format {
            LogFormat::Text => format_entries_text(&entries),
            LogFormat::Json => format_entries_json(&entries),
        }
    };

    ctx.handle
        .show_modal(format!("Session Log — {} events", entries.len()), vec![display_lines], None);

    Ok(SlashCommandControl::Continue)
}

struct LogEntry {
    seq: usize,
    event_type: &'static str,
    detail: String,
}

fn read_from_memory(thread_handle: &vtcode_core::core::threads::ThreadRuntimeHandle) -> Result<Vec<LogEntry>> {
    let records: Vec<ThreadEventRecord> = thread_handle.replay_recent();
    let mut entries = Vec::with_capacity(records.len());
    for record in records {
        let (event_type, detail) = describe_event(&record.event);
        entries.push(LogEntry { seq: record.sequence as usize, event_type, detail });
    }
    Ok(entries)
}

/// Scan on-disk session event logs for entries. Silently skips missing or
/// unreadable files — this is a best-effort supplement to the in-memory log.
///
/// # Blocking
///
/// Does `std::fs::read_dir` + per-file `File::open` + `read_line` — must be
/// called from `spawn_blocking` in async contexts.
fn collect_disk_entries(workspace: &std::path::Path) -> Vec<LogEntry> {
    let sessions_root = workspace.join(".vtcode/sessions");
    let mut entries = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(&sessions_root) else {
        return entries;
    };
    for entry in read_dir.filter_map(|entry| entry.ok()) {
        let path = entry.path().join("events.jsonl");
        if !path.exists() {
            continue;
        }
        if let Ok(disk_entries) = read_events_from_file(path) {
            entries.extend(disk_entries);
        }
    }
    entries
}

/// Read and parse a single `events.jsonl` file into log entries.
///
/// # Blocking
///
/// Does `std::fs::File::open` + `BufReader::lines` — must be called from
/// `spawn_blocking` in async contexts.
fn read_events_from_file(path: PathBuf) -> Result<Vec<LogEntry>> {
    let file = std::fs::File::open(&path).with_context(|| format!("Failed to open events log: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(versioned) = serde_json::from_str::<VersionedThreadEvent>(&line) else {
            continue;
        };
        let event = versioned.into_event();
        let (event_type, detail) = describe_event(&event);
        entries.push(LogEntry { seq: idx, event_type, detail });
    }
    Ok(entries)
}

fn item_type_label(details: &vtcode_core::exec::events::ThreadItemDetails) -> &'static str {
    use vtcode_core::exec::events::ThreadItemDetails;
    match details {
        ThreadItemDetails::Decision(_) => "Public decision",
        ThreadItemDetails::AgentMessage(_) => "AgentMessage",
        ThreadItemDetails::Plan(_) => "Plan",
        ThreadItemDetails::Reasoning(_) => "Reasoning",
        ThreadItemDetails::CommandExecution(_) => "CommandExecution",
        ThreadItemDetails::ToolInvocation(_) => "ToolInvocation",
        ThreadItemDetails::ToolOutput(_) => "ToolOutput",
        ThreadItemDetails::FileChange(_) => "FileChange",
        ThreadItemDetails::McpToolCall(_) => "McpToolCall",
        ThreadItemDetails::WebSearch(_) => "WebSearch",
        ThreadItemDetails::Harness(_) => "Harness",
        ThreadItemDetails::Error(_) => "Error",
    }
}

fn describe_event(event: &vtcode_core::exec::events::ThreadEvent) -> (&'static str, String) {
    match event {
        vtcode_core::exec::events::ThreadEvent::ThreadStarted(e) => {
            ("thread.started", format!("thread_id={}", e.thread_id))
        }
        vtcode_core::exec::events::ThreadEvent::ThreadCompleted(e) => (
            "thread.completed",
            format!("subtype={:?}, outcome={}, num_turns={}", e.subtype, e.outcome_code, e.num_turns),
        ),
        vtcode_core::exec::events::ThreadEvent::ThreadCompactBoundary(e) => {
            ("thread.compact_boundary", format!("trigger={:?}", e.trigger))
        }
        vtcode_core::exec::events::ThreadEvent::ContextReset(e) => (
            "context.reset",
            format!(
                "trigger={:?}, plan_preserved={}, previous_context={}%, tool_budget_reset={}",
                e.trigger, e.plan_preserved, e.previous_context_usage_percent, e.tool_budget_reset
            ),
        ),
        vtcode_core::exec::events::ThreadEvent::TurnStarted(_) => ("turn.started", String::new()),
        vtcode_core::exec::events::ThreadEvent::TurnCompleted(_) => ("turn.completed", String::new()),
        vtcode_core::exec::events::ThreadEvent::TurnFailed(e) => {
            ("turn.failed", truncate(&e.message, MAX_FIELD_DISPLAY_LEN).to_string())
        }
        vtcode_core::exec::events::ThreadEvent::TurnBlocked(e) => (
            "turn.blocked",
            format!(
                "{} (streak {}, total {})",
                truncate(&e.message, MAX_FIELD_DISPLAY_LEN),
                e.blocked_streak,
                e.blocked_total
            ),
        ),
        vtcode_core::exec::events::ThreadEvent::ItemStarted(e) => {
            ("item.started", format!("type={}", item_type_label(&e.item.details)))
        }
        vtcode_core::exec::events::ThreadEvent::ItemUpdated(e) => {
            ("item.updated", format!("type={}", item_type_label(&e.item.details)))
        }
        vtcode_core::exec::events::ThreadEvent::ItemCompleted(e) => {
            ("item.completed", format!("type={}", item_type_label(&e.item.details)))
        }
        vtcode_core::exec::events::ThreadEvent::PermissionRequested(e) => {
            ("permission.requested", format!("tool={}", truncate(&e.tool_name, MAX_FIELD_DISPLAY_LEN)))
        }
        vtcode_core::exec::events::ThreadEvent::PermissionResolved(e) => (
            "permission.resolved",
            format!(
                "tool={}, decision={:?}, wait_ms={}",
                truncate(&e.tool_name, MAX_FIELD_DISPLAY_LEN),
                e.decision,
                e.wait_ms
            ),
        ),
        vtcode_core::exec::events::ThreadEvent::Interjected(e) => {
            ("interjected", format!("source={:?}, redirect={:?}", e.source, e.redirect_kind))
        }
        vtcode_core::exec::events::ThreadEvent::PlanDelta(e) => {
            ("plan.delta", format!("thread_id={}, turn_id={}, item_id={}", e.thread_id, e.turn_id, e.item_id))
        }
        vtcode_core::exec::events::ThreadEvent::PlanApprovalRequested(e) => {
            ("plan.approval.requested", format!("thread_id={}, turn_id={}", e.thread_id, e.turn_id))
        }
        vtcode_core::exec::events::ThreadEvent::PlanApprovalResolved(e) => (
            "plan.approval.resolved",
            format!(
                "thread_id={}, turn_id={}, decision={:?}, automatic={}",
                e.thread_id, e.turn_id, e.decision, e.automatic
            ),
        ),
        vtcode_core::exec::events::ThreadEvent::Error(e) => {
            ("error", truncate(&e.message, MAX_FIELD_DISPLAY_LEN).to_string())
        }
        vtcode_core::exec::events::ThreadEvent::MatrixUpdated(snapshot) => {
            ("matrix.updated", format!("{}: {:?}", snapshot.spec.id, snapshot.lifecycle))
        }
        vtcode_core::exec::events::ThreadEvent::Unknown => ("unknown", String::new()),
    }
}

fn format_entries_text(entries: &[LogEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            let detail = if e.detail.is_empty() {
                String::new()
            } else {
                format!(" — {}", redact_secrets(e.detail.clone()))
            };
            format!("[#{:04}] {}{}", e.seq, e.event_type, detail)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_entries_json(entries: &[LogEntry]) -> String {
    let lines: Vec<String> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "seq": e.seq,
                "type": e.event_type,
                "detail": e.detail,
            })
            .to_string()
        })
        .collect();
    lines.join("\n")
}

fn save_log_file(workspace: &std::path::Path, content: &str, format: LogFormat) -> Result<PathBuf> {
    use chrono::Local;
    let ts = Local::now().format("%Y%m%d_%H%M%S");
    let ext = match format {
        LogFormat::Text => "txt",
        LogFormat::Json => "json",
    };
    let name = format!("vtcode-session-log-{}.{}", ts, ext);
    let path = workspace.join(name);
    write_file_with_context_sync(&path, content, "session log")?;
    Ok(path)
}

fn truncate(s: &str, max: usize) -> &str {
    let len = s.chars().count();
    if len <= max {
        s
    } else {
        s.split_at(s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len())).0
    }
}
