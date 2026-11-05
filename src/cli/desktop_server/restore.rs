use super::*;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RestoredInspectorHistory {
    pub(super) items: Vec<vtcode_core::exec::events::ThreadItem>,
    pub(super) incomplete: bool,
    pub(super) activity_items: Vec<vtcode_core::exec::events::ThreadItem>,
    pub(super) activity_incomplete: bool,
    pub(super) activity_output_cursors: Vec<protocol::SessionOutputCursor>,
    pub(super) output_spools: std::collections::HashMap<String, String>,
    pub(super) timeline_events: Vec<vtcode_core::exec::events::VersionedThreadEvent>,
    pub(super) timeline_incomplete: bool,
}

#[derive(Default)]
struct BoundedHistoryItems {
    pub(super) items: Vec<vtcode_core::exec::events::ThreadItem>,
    seen: std::collections::HashSet<String>,
    encoded_bytes: usize,
    pub(super) incomplete: bool,
    full: bool,
}

impl BoundedHistoryItems {
    fn retain_latest(&mut self, item: vtcode_core::exec::events::ThreadItem, byte_limit: usize) -> Result<()> {
        if self.full || !self.seen.insert(item.id.clone()) {
            return Ok(());
        }
        let size = serde_json::to_vec(&item)?.len();
        if self.items.len() >= 80 || self.encoded_bytes.saturating_add(size) > byte_limit {
            self.incomplete = true;
            self.full = true;
        } else {
            self.encoded_bytes += size;
            self.items.push(item);
        }
        Ok(())
    }
}

pub(super) struct RestoredSpoolText {
    pub(super) text: String,
    pub(super) truncated: bool,
    pub(super) total_bytes: u64,
    pub(super) pageable: bool,
}

pub(super) struct RestoredSpoolPage {
    pub(super) text: String,
    pub(super) next_offset: u64,
    pub(super) total_bytes: u64,
}

pub(super) struct RestoredOutputProjection {
    pub(super) incomplete: bool,
    pub(super) cursor: Option<protocol::SessionOutputCursor>,
    pub(super) spool_path: Option<String>,
}

/// Read a historical tool-output spool only from the workspace's canonical
/// tool-output directory. Event logs are durable, but spool files are bounded,
/// optional evidence and may have been removed since the session completed.
fn open_restored_spool_file(workspace: &Path, spool_path: &str) -> Option<std::fs::File> {
    let normalized = spool_path.replace('\\', "/");
    let has_foreign_drive_prefix = normalized.as_bytes().get(1) == Some(&b':');
    let path = Path::new(&normalized);
    if !path.is_absolute() && has_foreign_drive_prefix {
        return None;
    }
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace).ok()?
    } else {
        path
    };
    if !SpooledOutputReference::recognizes_file_path(relative) {
        return None;
    }

    let file = vtcode_commons::fs::bound_file::open_file_beneath(workspace, relative).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    Some(file)
}

pub(super) fn read_restored_spool_text(
    workspace: &Path,
    spool_path: &str,
    max_bytes: usize,
) -> Option<RestoredSpoolText> {
    if max_bytes == 0 {
        return None;
    }
    let mut file = open_restored_spool_file(workspace, spool_path)?;
    let total_bytes = file.metadata().ok()?.len();

    let mut bytes = Vec::with_capacity(max_bytes.min(MAX_RESTORED_SPOOLED_OUTPUT_BYTES));
    (&mut file).take(max_bytes as u64).read_to_end(&mut bytes).ok()?;
    let (visible_bytes, pageable) = match std::str::from_utf8(&bytes) {
        Ok(_) => (bytes.len(), true),
        Err(error) if error.error_len().is_none() && total_bytes > bytes.len() as u64 => (error.valid_up_to(), true),
        Err(_) => (bytes.len(), false),
    };
    let truncated = total_bytes > bytes.len() as u64 || !pageable || visible_bytes < bytes.len();
    let visible = if pageable { &bytes[..visible_bytes] } else { &bytes[..] };
    Some(RestoredSpoolText {
        text: String::from_utf8_lossy(visible).into_owned(),
        truncated,
        total_bytes,
        pageable,
    })
}

pub(super) fn read_restored_spool_page(workspace: &Path, spool_path: &str, offset: u64) -> Result<RestoredSpoolPage> {
    let mut file = open_restored_spool_file(workspace, spool_path)
        .ok_or_else(|| anyhow!("captured output is no longer available"))?;
    let total_bytes = file.metadata().context("failed to inspect captured output")?.len();
    if offset > total_bytes {
        bail!("captured output cursor is out of range");
    }
    file.seek(SeekFrom::Start(offset)).context("failed to seek captured output")?;
    let mut bytes = Vec::with_capacity(protocol::MAX_SESSION_OUTPUT_PAGE_BYTES);
    (&mut file)
        .take(protocol::MAX_SESSION_OUTPUT_PAGE_BYTES as u64)
        .read_to_end(&mut bytes)
        .context("failed to read captured output")?;
    let bytes_read = bytes.len() as u64;
    let reached_eof = offset.saturating_add(bytes_read) >= total_bytes;
    let (visible_bytes, has_incomplete_tail) = match std::str::from_utf8(&bytes) {
        Ok(_) => (bytes.len(), false),
        Err(error) if error.error_len().is_none() => (error.valid_up_to(), true),
        Err(_) => bail!("captured output is not valid UTF-8 text"),
    };
    if has_incomplete_tail && reached_eof {
        bail!("captured output ends with an incomplete UTF-8 sequence");
    }
    let consumed_bytes = if reached_eof && visible_bytes < bytes.len() {
        bytes_read
    } else {
        visible_bytes as u64
    };
    let next_offset = offset
        .checked_add(consumed_bytes)
        .ok_or_else(|| anyhow!("captured output cursor overflowed"))?;
    if next_offset == offset && offset < total_bytes {
        bail!("captured output could not advance at a UTF-8 boundary");
    }
    let text = std::str::from_utf8(&bytes[..visible_bytes])
        .context("captured output page ended at an invalid UTF-8 boundary")?
        .to_owned();
    Ok(RestoredSpoolPage { text, next_offset, total_bytes })
}

/// Restore bounded spool content into a historical ToolOutput item without
/// exposing its local path. A missing, stale, or unsafe reference leaves the
/// inline preview in place and marks inspector history incomplete.
pub(super) fn restore_spooled_tool_output(
    workspace: &Path,
    item: &mut vtcode_core::exec::events::ThreadItem,
    encoded_budget: usize,
) -> Result<RestoredOutputProjection> {
    use vtcode_core::exec::events::ThreadItemDetails;

    let (spool_path, preview) = {
        let ThreadItemDetails::ToolOutput(output) = &mut item.details else {
            return Ok(RestoredOutputProjection { incomplete: false, cursor: None, spool_path: None });
        };
        let Some(spool_path) = output.spool_path.take() else {
            return Ok(RestoredOutputProjection { incomplete: false, cursor: None, spool_path: None });
        };
        (spool_path, std::mem::take(&mut output.output))
    };
    let Some(spooled) = read_restored_spool_text(workspace, &spool_path, MAX_RESTORED_SPOOLED_OUTPUT_BYTES) else {
        if let ThreadItemDetails::ToolOutput(output) = &mut item.details {
            output.output = preview;
        }
        return Ok(RestoredOutputProjection { incomplete: true, cursor: None, spool_path: None });
    };

    let baseline_size = serde_json::to_vec(item)?.len();
    let mut prefix_bytes = spooled.text.len();
    let mut truncated = spooled.truncated;
    loop {
        let visible_truncation = truncated || prefix_bytes < spooled.text.len();
        let prefix_end = spooled.text.floor_char_boundary(prefix_bytes);
        let mut candidate = spooled.text.get(..prefix_end).unwrap_or_default().to_string();
        if visible_truncation {
            candidate.push_str(RESTORED_OUTPUT_TRUNCATION_SUFFIX);
        }
        if let ThreadItemDetails::ToolOutput(output) = &mut item.details {
            output.output = candidate;
        }
        let encoded_size = serde_json::to_vec(item)?.len();
        if encoded_size <= encoded_budget {
            let cursor = (spooled.pageable
                && prefix_end > 0
                && (prefix_end as u64) < spooled.total_bytes
                && item.id.len() <= 512)
                .then(|| protocol::SessionOutputCursor {
                    item_id: item.id.clone(),
                    next_offset: prefix_end as u64,
                    total_bytes: spooled.total_bytes,
                });
            return Ok(RestoredOutputProjection {
                incomplete: visible_truncation,
                spool_path: cursor.as_ref().map(|_| spool_path),
                cursor,
            });
        }
        if prefix_end == 0 {
            if let ThreadItemDetails::ToolOutput(output) = &mut item.details {
                output.output = preview;
            }
            return Ok(RestoredOutputProjection { incomplete: true, cursor: None, spool_path: None });
        }

        // JSON escaping can expand tool text. Scale the prefix to the remaining
        // encoded allowance, then retry at a UTF-8 boundary.
        let payload_size = encoded_size.saturating_sub(baseline_size);
        let payload_budget = encoded_budget.saturating_sub(baseline_size);
        prefix_bytes = prefix_end.saturating_mul(payload_budget) / payload_size.max(1);
        truncated = true;
    }
}

/// Read-only review projection over the shared canonical store. Lifecycle events
/// remain historical evidence and must never be replayed into the live runloop.
pub(super) fn restored_inspector_history(workspace: &Path, session_id: &str) -> Result<RestoredInspectorHistory> {
    use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails};
    const MAX_REVIEW_BYTES: usize = 200 * 1024;
    const MAX_ACTIVITY_BYTES: usize = 64 * 1024;
    let tail = match vtcode_memory::snapshot::read_event_tail(workspace, session_id, 4 * 1024 * 1024) {
        Ok(Some(tail)) => tail,
        Ok(None) => return Ok(RestoredInspectorHistory::default()),
        Err(vtcode_memory::SessionStoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::Unsupported =>
        {
            return Ok(RestoredInspectorHistory {
                incomplete: true,
                activity_incomplete: true,
                timeline_incomplete: true,
                ..Default::default()
            });
        }
        Err(error) => return Err(error.into()),
    };
    let mut review = BoundedHistoryItems { incomplete: tail.truncated, ..Default::default() };
    let mut activity = BoundedHistoryItems { incomplete: tail.truncated, ..Default::default() };
    let mut activity_output_cursors = Vec::new();
    let mut output_spools = std::collections::HashMap::new();
    let (timeline_events, timeline_incomplete) = bounded_timeline(&tail.events, tail.truncated)?;
    for event in tail.events.into_iter().rev() {
        let mut item = match event.into_event() {
            ThreadEvent::ItemStarted(event) => event.item,
            ThreadEvent::ItemUpdated(event) => event.item,
            ThreadEvent::ItemCompleted(event) => event.item,
            _ => continue,
        };

        if matches!(&item.details, ThreadItemDetails::FileChange(_)) {
            review.retain_latest(item, MAX_REVIEW_BYTES)?;
            continue;
        }
        if matches!(&item.details, ThreadItemDetails::ToolOutput(_)) {
            let mut restored_output = RestoredOutputProjection { incomplete: false, cursor: None, spool_path: None };
            if !activity.full && !activity.seen.contains(&item.id) {
                let remaining = MAX_ACTIVITY_BYTES.saturating_sub(activity.encoded_bytes);
                restored_output = restore_spooled_tool_output(workspace, &mut item, remaining)?;
                activity.incomplete |= restored_output.incomplete;
            }
            let item_id = item.id.clone();
            activity.retain_latest(item, MAX_ACTIVITY_BYTES)?;
            if activity.items.last().is_some_and(|item| item.id == item_id) {
                if let (Some(cursor), Some(spool_path)) = (restored_output.cursor, restored_output.spool_path) {
                    output_spools.insert(cursor.item_id.clone(), spool_path);
                    activity_output_cursors.push(cursor);
                }
            }
            continue;
        }
        if matches!(
            &item.details,
            ThreadItemDetails::ToolInvocation(_)
                | ThreadItemDetails::CommandExecution(_)
                | ThreadItemDetails::McpToolCall(_)
                | ThreadItemDetails::WebSearch(_)
                | ThreadItemDetails::Harness(_)
                | ThreadItemDetails::Error(_)
        ) {
            activity.retain_latest(item, MAX_ACTIVITY_BYTES)?;
        }
    }
    review.items.reverse();
    activity.items.reverse();
    activity_output_cursors.reverse();
    Ok(RestoredInspectorHistory {
        items: review.items,
        incomplete: review.incomplete,
        activity_items: activity.items,
        activity_incomplete: activity.incomplete,
        activity_output_cursors,
        output_spools,
        timeline_events,
        timeline_incomplete,
    })
}

fn bounded_timeline(
    events: &[vtcode_core::exec::events::VersionedThreadEvent],
    truncated: bool,
) -> Result<(Vec<vtcode_core::exec::events::VersionedThreadEvent>, bool)> {
    let mut retained = Vec::new();
    let mut bytes = 0_usize;
    let mut incomplete = truncated;
    for event in events.iter().rev() {
        let size = serde_json::to_vec(event)?.len();
        if retained.len() >= 512 || bytes.saturating_add(size) > 256 * 1024 {
            incomplete = true;
            break;
        }
        bytes += size;
        retained.push(event.clone());
    }
    retained.reverse();
    Ok((retained, incomplete))
}

pub(super) struct RestoredHistoryPage {
    pub(super) start_index: usize,
    pub(super) messages: Vec<protocol::SessionHistoryMessage>,
    pub(super) has_earlier: bool,
}

pub(super) fn restored_history_page(
    archived_messages: &[vtcode_core::utils::session_archive::SessionMessage],
    before_index: Option<usize>,
    limit: usize,
) -> RestoredHistoryPage {
    let end = before_index.unwrap_or(archived_messages.len()).min(archived_messages.len());
    let page_start = end.saturating_sub(limit);
    let mut messages = Vec::new();
    let mut total_bytes = 0_usize;
    let mut start_index = end;

    for (archive_index, message) in archived_messages.iter().enumerate().take(end).skip(page_start).rev() {
        let content = message.content.as_text();
        let hidden =
            vtcode_core::utils::session_archive::is_internal_session_context(message.role, &content, message.clear_at);
        let text = if hidden {
            String::new()
        } else {
            bounded_text_projection(&content, MAX_RESTORED_MESSAGE_BYTES)
        };
        let restored = protocol::SessionHistoryMessage {
            hidden,
            archive_index: Some(archive_index),
            role: message.role.to_string(),
            event_item_id: message
                .metadata
                .as_deref()
                .and_then(|metadata| metadata.event_item_id())
                .filter(|id| !id.is_empty() && id.len() <= 512)
                .map(str::to_owned),
            phase: message.phase.map(|phase| match phase {
                vtcode_core::llm::provider::AssistantPhase::Commentary => "commentary".to_owned(),
                vtcode_core::llm::provider::AssistantPhase::FinalAnswer => "final_answer".to_owned(),
            }),
            text,
        };
        let encoded_size = serde_json::to_vec(&restored).map_or(MAX_RESTORED_MESSAGE_BYTES, |bytes| bytes.len());
        if total_bytes.saturating_add(encoded_size) > MAX_RESTORED_HISTORY_BYTES {
            break;
        }
        total_bytes = total_bytes.saturating_add(encoded_size);
        start_index = archive_index;
        messages.push(restored);
    }

    messages.reverse();
    RestoredHistoryPage {
        start_index,
        messages,
        has_earlier: start_index > 0,
    }
}

fn bounded_text_projection(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let end = text.floor_char_boundary(max_bytes);
    let mut bounded = text[..end].to_string();
    bounded.push_str("\n[Earlier content in this message was omitted.]");
    bounded
}

#[cfg(test)]
mod tests {
    use super::{bounded_text_projection, bounded_timeline};

    #[test]
    fn bounded_timeline_keeps_newest_contiguous_events_in_order() {
        use vtcode_core::exec::events::{ThreadEvent, ThreadStartedEvent, VersionedThreadEvent};
        let events: Vec<_> = (0..520)
            .map(|index| {
                VersionedThreadEvent::new(ThreadEvent::ThreadStarted(ThreadStartedEvent {
                    thread_id: index.to_string(),
                }))
            })
            .collect();
        let (retained, incomplete) = bounded_timeline(&events, false).unwrap();
        assert!(incomplete);
        assert_eq!(retained.len(), 512);
        assert!(
            matches!(retained.first().unwrap().event(), ThreadEvent::ThreadStarted(event) if event.thread_id == "8")
        );
        assert!(
            matches!(retained.last().unwrap().event(), ThreadEvent::ThreadStarted(event) if event.thread_id == "519")
        );
        let (all, incomplete) = bounded_timeline(&events[..2], true).unwrap();
        assert_eq!(all.len(), 2);
        assert!(incomplete);
    }

    #[test]
    fn bounded_projection_cuts_multibyte_text_on_a_char_boundary() {
        const MARKER: &str = "\n[Earlier content in this message was omitted.]";
        // `世` is 3 bytes: a 4-byte limit falls inside the second char.
        assert_eq!(bounded_text_projection("世界", 4), format!("世{MARKER}"));
        assert_eq!(bounded_text_projection("世界", 6), "世界");
        assert_eq!(bounded_text_projection("世界", 2), MARKER);
    }
}
