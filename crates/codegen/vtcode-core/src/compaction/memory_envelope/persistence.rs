use super::*;

/// Persist the recoverable full-history artifact and build + inject a session
/// memory envelope into the compacted history. Returns the envelope (with the
/// artifact path) when one was produced.
pub fn persist_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    persist_memory_envelope_with_update(
        workspace_root,
        session_id,
        vt_cfg,
        original_history,
        touched_files,
        compacted,
        persistence,
        placement,
        seed_envelope,
        None,
    )
}

/// Variant of [`persist_memory_envelope`] that applies a live session update
/// while constructing the envelope. Keeping the compatibility wrapper above
/// avoids changing synchronous callers that do not track steering state.
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub fn persist_memory_envelope_with_update(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    let should_persist = should_persist_memory_envelope(vt_cfg);
    if original_history.is_empty() || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk) {
        return Ok(None);
    }

    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let history_artifact_path = if should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk {
        let mut hm = HistoryFileManager::new(workspace_root, session_id);
        let hm2 = messages_to_history_messages(original_history, 0);
        let hr = hm
            .write_history_sync(&hm2, original_history.len(), "compaction", touched_files, &[])
            .context("write compaction history artifact")?;
        Some(hr.file_path)
    } else {
        None
    };
    let loaded = if seed_envelope.is_none() {
        load_latest_memory_envelope(workspace_root, session_id)
    } else {
        None
    };
    let prior = seed_envelope.or(loaded.as_ref());
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        original_history,
        touched_files,
        extract_compaction_summary(compacted, original_history),
        history_artifact_path.as_deref(),
        prior,
        &task_snapshot,
        envelope_update,
    );

    if let Some(hap) = history_artifact_path.as_ref() {
        write_memory_envelope_to_path(&memory_envelope_path_from_history_path(workspace_root, hap), &envelope)?;
    }
    apply_memory_envelope(compacted, &envelope, placement);
    Ok(Some(envelope))
}

/// Async counterpart to [`persist_memory_envelope`]. Compaction runs on the
/// async agent loop, so filesystem work must not block the runtime while the
/// recoverable history artifact or envelope is being written.
pub async fn persist_memory_envelope_async(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    persist_memory_envelope_async_with_update(
        workspace_root,
        session_id,
        vt_cfg,
        original_history,
        touched_files,
        compacted,
        persistence,
        placement,
        seed_envelope,
        None,
    )
    .await
}

/// Async variant of [`persist_memory_envelope_async`] that applies a live
/// session update while constructing the envelope.
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub async fn persist_memory_envelope_async_with_update(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    let should_persist = should_persist_memory_envelope(vt_cfg);
    if original_history.is_empty() || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk) {
        return Ok(None);
    }

    let task_snapshot = read_task_tracker_snapshot_async(workspace_root).await;
    let history_artifact_path = if should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk {
        let mut history_manager = HistoryFileManager::new(workspace_root, session_id);
        let history_messages = messages_to_history_messages(original_history, 0);
        let history_result = history_manager
            .write_history(&history_messages, original_history.len(), "compaction", touched_files, &[])
            .await
            .context("write compaction history artifact")?;
        Some(history_result.file_path)
    } else {
        None
    };

    let loaded = if seed_envelope.is_none() {
        load_latest_memory_envelope_async(workspace_root, session_id).await
    } else {
        None
    };
    let prior = seed_envelope.or(loaded.as_ref());
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        original_history,
        touched_files,
        extract_compaction_summary(compacted, original_history),
        history_artifact_path.as_deref(),
        prior,
        &task_snapshot,
        envelope_update,
    );

    if let Some(history_path) = history_artifact_path.as_ref() {
        write_memory_envelope_to_path_async(
            &memory_envelope_path_from_history_path(workspace_root, history_path),
            &envelope,
        )
        .await?;
    }
    apply_memory_envelope(compacted, &envelope, placement);
    Ok(Some(envelope))
}

pub fn should_persist_memory_envelope(vt_cfg: Option<&VTCodeConfig>) -> bool {
    vt_cfg.is_some_and(|cfg| cfg.context.dynamic.enabled && cfg.context.dynamic.persist_history)
}

fn memory_envelope_message(envelope: &SessionMemoryEnvelope) -> Message {
    let mut sections = Vec::new();
    sections.push(format!("{}\nSummary:\n{}", MEMORY_ENVELOPE_HEADER, envelope.summary.trim()));

    fn maybe_section(prefix: &str, content: Option<&str>) -> Option<String> {
        content
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("{prefix}\n{s}"))
    }

    fn list_section(prefix: &str, items: &[String]) -> Option<String> {
        (!items.is_empty()).then(|| format!("{prefix}\n- {}", items.join("\n- ")))
    }

    if let Some(s) = maybe_section("Objective", envelope.objective.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Task Tracker", envelope.task_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Spec Summary", envelope.spec_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Evaluation Summary", envelope.evaluation_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Verification Status", envelope.verification_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = list_section("Constraints", &envelope.constraints) {
        sections.push(s);
    }
    if let Some(s) = list_section("Touched Files", &envelope.touched_files) {
        sections.push(s);
    }

    if !envelope.grounded_facts.is_empty() {
        let facts: Vec<_> = envelope
            .grounded_facts
            .iter()
            .map(|f| format!("[{}] {}", f.source, f.fact.trim()))
            .collect();
        sections.push(format!("Grounded Facts:\n{}", facts.join("\n")));
    }
    if let Some(s) = list_section("Open Questions", &envelope.open_questions) {
        sections.push(s);
    }
    if let Some(s) = list_section("Verification Todo", &envelope.verification_todo) {
        sections.push(s);
    }
    if let Some(s) = list_section("Delegation Notes", &envelope.delegation_notes) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("History Artifact", envelope.history_artifact_path.as_deref()) {
        sections.push(s);
    }

    Message::system(sections.join("\n\n"))
}

fn is_compaction_summary_message(message: &Message) -> bool {
    message.role == MessageRole::System && message.content.as_text().starts_with("Previous conversation summary:\n")
}

pub fn strip_existing_memory_envelope(history: &mut Vec<Message>) {
    history.retain(|message| {
        !(message.role == MessageRole::System && message.content.as_text().starts_with(MEMORY_ENVELOPE_HEADER))
    });
}

pub(super) fn extract_compaction_summary(compacted: &[Message], original_history: &[Message]) -> String {
    if let Some(summary) = compacted.iter().find_map(|message| {
        if message.role != MessageRole::System {
            return None;
        }

        let text = message.content.as_text();
        let trimmed = text.trim();
        trimmed
            .strip_prefix("Previous conversation summary:\n")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        return summary;
    }

    if let Some(summary) = compacted.iter().find_map(|message| {
        if message.role != MessageRole::Assistant {
            return None;
        }

        message.reasoning_details.as_ref()?.iter().find_map(|detail| {
            let value = match detail {
                Value::String(serialized) => serde_json::from_str::<Value>(serialized).ok()?,
                value => value.clone(),
            };
            let summary = value
                .get("content")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|summary| !summary.is_empty())?;
            (value.get("type").and_then(Value::as_str) == Some("compaction")).then_some(summary.to_string())
        })
    }) {
        return summary;
    }

    let mut recent = original_history
        .iter()
        .rev()
        .filter_map(|message| {
            let text = message.content.as_text();
            let trimmed = normalize_whitespace(text.as_ref());
            (!trimmed.is_empty()).then_some(format!(
                "{}: {}",
                message.role.as_generic_str(),
                truncate_for_fact(&trimmed, 160)
            ))
        })
        .take(4)
        .collect::<Vec<_>>();
    recent.reverse();

    if recent.is_empty() {
        "Compacted earlier conversation state and preserved continuity facts.".to_string()
    } else {
        format!("Compacted earlier conversation state. Recent preserved context: {}", recent.join(" | "))
    }
}

/// Sanitize a session id the same way history envelope filenames do
/// (32-char ASCII-safe prefix). Callers that match envelope names must use
/// this, not the raw session id.
pub fn sanitize_session_id(session_id: &str) -> String {
    session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect()
}

/// Whether a history envelope filename belongs to `session_id`.
///
/// Envelope names use the 32-char sanitized id, with optional `_<n>`
/// suffixes. Matching must use this exact rule — a loose `starts_with` on
/// the raw id over-preserves unrelated sessions.
pub fn memory_envelope_file_matches_session(name: &str, session_id: &str) -> bool {
    let session_prefix = sanitize_session_id(session_id);
    name == format!("{session_prefix}{MEMORY_ENVELOPE_SUFFIX}")
        || (name.starts_with(&format!("{session_prefix}_")) && name.ends_with(MEMORY_ENVELOPE_SUFFIX))
}

pub(super) fn parse_task_tracker_snapshot(content: &str) -> TaskTrackerSnapshot {
    let title = content
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line.trim_start_matches("# ").trim().to_string());
    let checklist = content
        .lines()
        .filter(|line| line.trim_start().starts_with("- ["))
        .take(5)
        .map(normalize_whitespace)
        .collect::<Vec<_>>();
    let verification_summary = extract_verification_summary(content, &checklist);
    let verification_todo = content
        .lines()
        .filter(|line| line.trim_start().starts_with("- [ ]"))
        .take(MEMORY_LIST_LIMIT)
        .map(normalize_whitespace)
        .collect::<Vec<_>>();
    let summary = match (title.clone(), checklist.is_empty()) {
        (Some(title), false) => Some(format!("{title}: {}", checklist.join(" | "))),
        (Some(title), true) => Some(title),
        (None, false) => Some(checklist.join(" | ")),
        (None, true) => None,
    };

    TaskTrackerSnapshot {
        summary,
        objective: title,
        verification_summary,
        verification_todo,
    }
}

pub fn read_task_tracker_snapshot(workspace_root: &Path) -> TaskTrackerSnapshot {
    let tracker_path = current_task_path(workspace_root);
    fs::read_to_string(&tracker_path)
        .ok()
        .map(|content| parse_task_tracker_snapshot(&content))
        .unwrap_or_default()
}

/// Read the task tracker without blocking the async runtime.
pub async fn read_task_tracker_snapshot_async(workspace_root: &Path) -> TaskTrackerSnapshot {
    let tracker_path = current_task_path(workspace_root);
    async_fs::read_to_string(tracker_path)
        .await
        .ok()
        .map(|content| parse_task_tracker_snapshot(&content))
        .unwrap_or_default()
}

pub(super) fn extract_verification_summary(content: &str, checklist: &[String]) -> Option<String> {
    let verify_commands = collect_structured_verify_commands(content);
    if !verify_commands.is_empty() {
        return Some(render_bullet_list(&verify_commands));
    }

    let fallback_lines = checklist
        .iter()
        .filter(|line| looks_like_verification_line(line))
        .cloned()
        .collect::<Vec<_>>();
    (!fallback_lines.is_empty()).then(|| fallback_lines.join("\n"))
}

/// Split the tail of a `verify:` line into clean commands.
///
/// Plan writers sometimes emit `verify: [a] and verify: [b]` on one line.
/// Strip at most one matching outer `[`…`]` pair per piece and drop empties so
/// a spliced marker can never leak into `verification_summary`.
pub(super) fn split_verify_command_tail(rest: &str) -> Vec<String> {
    rest.split(" and verify:")
        .map(str::trim)
        .map(strip_one_outer_bracket_pair)
        .filter(|piece| !piece.is_empty())
        .map(normalize_whitespace)
        .filter(|piece| !piece.is_empty())
        .collect()
}

fn strip_one_outer_bracket_pair(piece: &str) -> &str {
    let trimmed = piece.trim();
    // Plan splices can leave the closing bracket off (`verify: [cmd`).
    let Some(inner) = trimmed.strip_prefix('[').map(str::trim_start) else {
        return trimmed;
    };
    match inner.strip_suffix(']') {
        Some(stripped) => stripped.trim_end(),
        None => inner,
    }
}

pub(super) fn collect_structured_verify_commands(content: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_verify_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("verify:") {
            for command in split_verify_command_tail(rest) {
                commands.push(command);
            }
            in_verify_block = rest.trim().is_empty();
            continue;
        }

        if !in_verify_block {
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        if (line.starts_with(' ') || line.starts_with('\t')) && trimmed.starts_with("- ") {
            commands.push(normalize_whitespace(trimmed.trim_start_matches("- ")));
            continue;
        }

        in_verify_block = false;
    }

    commands
}

fn render_bullet_list(items: &[String]) -> String {
    items.iter().map(|item| format!("- {item}")).collect::<Vec<_>>().join("\n")
}

fn looks_like_verification_line(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    [
        "verify",
        "verification",
        "test",
        "lint",
        "cargo check",
        "check-dev.sh",
        "check.sh",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
}

fn memory_envelope_path_from_history_path(workspace_root: &Path, history_path: &Path) -> PathBuf {
    let absolute_history_path = if history_path.is_absolute() {
        history_path.to_path_buf()
    } else {
        workspace_root.join(history_path)
    };

    let file_name = absolute_history_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            if let Some(stem) = name.strip_suffix(".jsonl") {
                format!("{stem}{MEMORY_ENVELOPE_SUFFIX}")
            } else {
                format!("{name}{MEMORY_ENVELOPE_SUFFIX}")
            }
        })
        .unwrap_or_else(|| format!("session_memory{MEMORY_ENVELOPE_SUFFIX}"));

    let parent = absolute_history_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| workspace_root.join(".vtcode").join("history"));
    parent.join(file_name)
}

pub fn default_memory_envelope_path_for_session(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".vtcode")
        .join("history")
        .join(format!("{}{MEMORY_ENVELOPE_SUFFIX}", sanitize_session_id(session_id)))
}

fn memory_envelope_paths_for_session(workspace_root: &Path, session_id: &str) -> Vec<PathBuf> {
    let history_dir = workspace_root.join(".vtcode").join("history");
    let mut candidates = fs::read_dir(history_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| memory_envelope_file_matches_session(name, session_id))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        let left_modified = fs::metadata(left).and_then(|metadata| metadata.modified()).ok();
        let right_modified = fs::metadata(right).and_then(|metadata| metadata.modified()).ok();
        right_modified
            .cmp(&left_modified)
            .then_with(|| right.file_name().cmp(&left.file_name()))
    });
    candidates
}

pub fn latest_memory_envelope_path_for_session(workspace_root: &Path, session_id: &str) -> Option<PathBuf> {
    memory_envelope_paths_for_session(workspace_root, session_id)
        .into_iter()
        .find(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str::<SessionMemoryEnvelope>(&content).ok())
                .is_some_and(|envelope| envelope.session_id.is_empty() || envelope.session_id == session_id)
        })
}

pub fn load_latest_memory_envelope(workspace_root: &Path, session_id: &str) -> Option<SessionMemoryEnvelope> {
    let path = latest_memory_envelope_path_for_session(workspace_root, session_id)?;
    let content = fs::read_to_string(path).ok()?;
    let envelope: SessionMemoryEnvelope = serde_json::from_str(&content).ok()?;
    if !envelope.session_id.is_empty() && envelope.session_id != session_id {
        return None;
    }
    Some(envelope)
}

async fn memory_envelope_paths_for_session_async(workspace_root: &Path, session_id: &str) -> Vec<PathBuf> {
    let history_dir = workspace_root.join(".vtcode").join("history");
    let mut candidates = Vec::new();
    let Ok(mut entries) = async_fs::read_dir(history_dir).await else {
        return candidates;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let matches = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| memory_envelope_file_matches_session(name, session_id));
        if matches {
            candidates.push(path);
        }
    }

    let mut modified = Vec::with_capacity(candidates.len());
    for path in candidates {
        let timestamp = async_fs::metadata(&path)
            .await
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        modified.push((timestamp, path));
    }
    modified.sort_by(|(left_time, left_path), (right_time, right_path)| {
        right_time
            .cmp(left_time)
            .then_with(|| right_path.file_name().cmp(&left_path.file_name()))
    });
    modified.into_iter().map(|(_, path)| path).collect()
}

/// Find and deserialize the newest valid envelope without synchronously
/// scanning historical candidates on the runtime thread.
pub async fn latest_memory_envelope_path_for_session_async(workspace_root: &Path, session_id: &str) -> Option<PathBuf> {
    for path in memory_envelope_paths_for_session_async(workspace_root, session_id).await {
        let Ok(content) = async_fs::read_to_string(&path).await else {
            continue;
        };
        let Ok(envelope) = serde_json::from_str::<SessionMemoryEnvelope>(&content) else {
            continue;
        };
        if envelope.session_id.is_empty() || envelope.session_id == session_id {
            return Some(path);
        }
    }
    None
}

pub async fn load_latest_memory_envelope_async(
    workspace_root: &Path,
    session_id: &str,
) -> Option<SessionMemoryEnvelope> {
    let path = latest_memory_envelope_path_for_session_async(workspace_root, session_id).await?;
    let content = async_fs::read_to_string(path).await.ok()?;
    let envelope: SessionMemoryEnvelope = serde_json::from_str(&content).ok()?;
    if !envelope.session_id.is_empty() && envelope.session_id != session_id {
        return None;
    }
    Some(envelope)
}

pub fn insert_memory_envelope_message(
    history: &mut Vec<Message>,
    envelope: &SessionMemoryEnvelope,
    placement: MemoryEnvelopePlacement,
) {
    let message = memory_envelope_message(envelope);
    match placement {
        MemoryEnvelopePlacement::Start => history.insert(0, message),
        MemoryEnvelopePlacement::BeforeLastUserOrSummary => {
            let insert_at = history
                .iter()
                .rposition(|item| item.role == MessageRole::User || is_compaction_summary_message(item))
                .unwrap_or(0);
            history.insert(insert_at, message);
        }
    }
}

fn apply_memory_envelope(
    compacted: &mut Vec<Message>,
    envelope: &SessionMemoryEnvelope,
    placement: MemoryEnvelopePlacement,
) {
    strip_existing_memory_envelope(compacted);
    insert_memory_envelope_message(compacted, envelope, placement);
}

pub fn inject_latest_memory_envelope(workspace_root: &Path, session_id: &str, history: &mut Vec<Message>) -> bool {
    let Some(envelope) = load_latest_memory_envelope(workspace_root, session_id) else {
        return false;
    };

    strip_existing_memory_envelope(history);
    insert_memory_envelope_message(history, &envelope, MemoryEnvelopePlacement::Start);
    true
}

pub fn write_memory_envelope_to_path(path: &Path, envelope: &SessionMemoryEnvelope) -> anyhow::Result<()> {
    let serialized = serde_json::to_vec_pretty(envelope).context("serialize memory envelope")?;
    vtcode_commons::VtCodePaths::write_private_file_atomic(path, &serialized)
        .with_context(|| format!("write memory envelope {}", path.display()))
}

/// Write an envelope through the shared private-file policy on a blocking worker.
/// The synced, exclusive temporary file is created beside the destination and
/// renamed over it without first removing the last committed envelope.
pub async fn write_memory_envelope_to_path_async(path: &Path, envelope: &SessionMemoryEnvelope) -> anyhow::Result<()> {
    let serialized = serde_json::to_vec_pretty(envelope).context("serialize memory envelope")?;
    vtcode_commons::fs::write_private_file_atomic(path, &serialized)
        .await
        .with_context(|| format!("write memory envelope {}", path.display()))
}

pub fn has_latest_memory_envelope(workspace_root: &Path, session_id: &str) -> bool {
    latest_memory_envelope_path_for_session(workspace_root, session_id).is_some()
}

// ---------------------------------------------------------------------------
// Local compaction configuration + zero-cost fork history
// ---------------------------------------------------------------------------

pub fn configured_retained_user_messages(vt_cfg: Option<&VTCodeConfig>) -> usize {
    vt_cfg.map(|cfg| cfg.context.dynamic.retained_user_messages).unwrap_or(4)
}

pub fn local_compaction_config(vt_cfg: Option<&VTCodeConfig>, always_summarize: bool) -> CompactionConfig {
    CompactionConfig {
        always_summarize,
        retained_user_messages: configured_retained_user_messages(vt_cfg),
        ..CompactionConfig::default()
    }
}
