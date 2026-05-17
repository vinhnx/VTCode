use super::*;

pub(super) fn should_persist_memory_envelope(vt_cfg: Option<&VTCodeConfig>) -> bool {
    vt_cfg.is_some_and(|cfg| cfg.context.dynamic.enabled && cfg.context.dynamic.persist_history)
}

fn memory_envelope_message(envelope: &SessionMemoryEnvelope) -> Message {
    let mut sections = Vec::new();
    sections.push(format!(
        "{}\nSummary:\n{}",
        MEMORY_ENVELOPE_HEADER,
        envelope.summary.trim()
    ));

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
    if let Some(s) = maybe_section(
        "History Artifact",
        envelope.history_artifact_path.as_deref(),
    ) {
        sections.push(s);
    }

    Message::system(sections.join("\n\n"))
}

fn is_compaction_summary_message(message: &Message) -> bool {
    message.role == MessageRole::System
        && message
            .content
            .as_text()
            .starts_with("Previous conversation summary:\n")
}

pub(crate) fn strip_existing_memory_envelope(history: &mut Vec<Message>) {
    history.retain(|message| {
        !(message.role == MessageRole::System
            && message
                .content
                .as_text()
                .starts_with(MEMORY_ENVELOPE_HEADER))
    });
}

pub(super) fn extract_compaction_summary(
    compacted: &[Message],
    original_history: &[Message],
) -> String {
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
        format!(
            "Compacted earlier conversation state. Recent preserved context: {}",
            recent.join(" | ")
        )
    }
}

fn sanitize_session_id(session_id: &str) -> String {
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

fn memory_envelope_file_matches_session(name: &str, session_id: &str) -> bool {
    let session_prefix = sanitize_session_id(session_id);
    name == format!("{session_prefix}{MEMORY_ENVELOPE_SUFFIX}")
        || (name.starts_with(&format!("{session_prefix}_"))
            && name.ends_with(MEMORY_ENVELOPE_SUFFIX))
}

pub(super) fn read_task_tracker_snapshot(workspace_root: &Path) -> TaskTrackerSnapshot {
    let tracker_path = current_task_path(workspace_root);
    let Ok(content) = fs::read_to_string(&tracker_path) else {
        return TaskTrackerSnapshot::default();
    };

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
        verification_todo,
    }
}

pub(super) fn memory_envelope_path_from_history_path(
    workspace_root: &Path,
    history_path: &Path,
) -> PathBuf {
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

pub(super) fn default_memory_envelope_path_for_session(
    workspace_root: &Path,
    session_id: &str,
) -> PathBuf {
    workspace_root.join(".vtcode").join("history").join(format!(
        "{}{MEMORY_ENVELOPE_SUFFIX}",
        sanitize_session_id(session_id)
    ))
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
    candidates.sort();
    candidates
}

pub(crate) fn latest_memory_envelope_path_for_session(
    workspace_root: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    memory_envelope_paths_for_session(workspace_root, session_id)
        .into_iter()
        .rev()
        .find(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str::<SessionMemoryEnvelope>(&content).ok())
                .is_some_and(|envelope| {
                    envelope.session_id.is_empty() || envelope.session_id == session_id
                })
        })
}

pub(crate) fn load_latest_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
) -> Option<SessionMemoryEnvelope> {
    let path = latest_memory_envelope_path_for_session(workspace_root, session_id)?;
    let content = fs::read_to_string(path).ok()?;
    let envelope: SessionMemoryEnvelope = serde_json::from_str(&content).ok()?;
    if !envelope.session_id.is_empty() && envelope.session_id != session_id {
        return None;
    }
    Some(envelope)
}

pub(crate) fn insert_memory_envelope_message(
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
                .rposition(|item| {
                    item.role == MessageRole::User || is_compaction_summary_message(item)
                })
                .unwrap_or(0);
            history.insert(insert_at, message);
        }
    }
}

pub(super) fn apply_memory_envelope(
    compacted: &mut Vec<Message>,
    envelope: &SessionMemoryEnvelope,
    placement: MemoryEnvelopePlacement,
) {
    strip_existing_memory_envelope(compacted);
    insert_memory_envelope_message(compacted, envelope, placement);
}

pub(crate) fn inject_latest_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    history: &mut Vec<Message>,
) -> bool {
    let Some(envelope) = load_latest_memory_envelope(workspace_root, session_id) else {
        return false;
    };

    strip_existing_memory_envelope(history);
    insert_memory_envelope_message(history, &envelope, MemoryEnvelopePlacement::Start);
    true
}

pub(super) fn write_memory_envelope_to_path(
    path: &Path,
    envelope: &SessionMemoryEnvelope,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create memory envelope directory {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(envelope)?;
    fs::write(path, serialized)
        .with_context(|| format!("write memory envelope {}", path.display()))?;
    Ok(())
}

pub(crate) fn has_latest_memory_envelope(workspace_root: &Path, session_id: &str) -> bool {
    latest_memory_envelope_path_for_session(workspace_root, session_id).is_some()
}
