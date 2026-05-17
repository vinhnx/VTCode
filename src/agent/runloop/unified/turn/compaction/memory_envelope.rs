use super::*;
mod local_summary;
mod persistence;

pub(super) use self::local_summary::{
    build_zero_cost_summarized_fork_history, configured_retained_user_messages,
    local_compaction_config,
};
use self::persistence::{
    apply_memory_envelope, default_memory_envelope_path_for_session, extract_compaction_summary,
    memory_envelope_path_from_history_path, read_task_tracker_snapshot,
    should_persist_memory_envelope, write_memory_envelope_to_path,
};
pub(crate) use self::persistence::{
    has_latest_memory_envelope, inject_latest_memory_envelope,
    latest_memory_envelope_path_for_session,
};
pub(super) use self::persistence::{
    insert_memory_envelope_message, load_latest_memory_envelope, strip_existing_memory_envelope,
};

fn merge_dedup_push<T, K, F>(
    prior: &[T],
    updates: impl IntoIterator<Item = T>,
    limit: usize,
    key_fn: F,
) -> Vec<T>
where
    K: PartialEq,
    F: Fn(&T) -> K,
    T: Clone,
{
    let mut merged = prior.to_vec();
    for item in updates {
        if let Some(idx) = merged.iter().position(|e| key_fn(e) == key_fn(&item)) {
            merged.remove(idx);
        }
        merged.push(item);
    }
    let keep_from = merged.len().saturating_sub(limit);
    merged.into_iter().skip(keep_from).collect()
}

fn merge_touched_files(
    prior_envelope: Option<&SessionMemoryEnvelope>,
    touched_files: &[String],
) -> Vec<String> {
    let prior = prior_envelope
        .map(|e| e.touched_files.as_slice())
        .unwrap_or(&[]);
    merge_dedup_push(prior, touched_files.iter().cloned(), usize::MAX, |s| {
        s.clone()
    })
}

fn merge_recent_strings(prior: &[String], updates: &[String], limit: usize) -> Vec<String> {
    let prior_normalized: Vec<_> = prior
        .iter()
        .map(|v| normalize_whitespace(v))
        .filter(|v| !v.is_empty())
        .collect();
    let updates_normalized: Vec<_> = updates
        .iter()
        .map(|v| normalize_whitespace(v))
        .filter(|v| !v.is_empty())
        .collect();
    merge_dedup_push(&prior_normalized, updates_normalized, limit, |s| {
        s.to_ascii_lowercase()
    })
}

fn extract_constraints_from_summary(text: Option<&str>) -> Vec<String> {
    text.into_iter()
        .flat_map(|value| value.lines())
        .map(normalize_whitespace)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            if let Some(rest) = line.strip_prefix("- ") {
                return Some(rest.trim().to_string());
            }
            line.strip_prefix("* ").map(|rest| rest.trim().to_string())
        })
        .take(MEMORY_LIST_LIMIT)
        .collect()
}

fn derive_continuity_summary(
    history: &[Message],
    prior_envelope: Option<&SessionMemoryEnvelope>,
) -> String {
    let mut recent = history
        .iter()
        .rev()
        .filter(|message| {
            !(message.role == MessageRole::System
                && message
                    .content
                    .as_text()
                    .starts_with(MEMORY_ENVELOPE_HEADER))
        })
        .filter_map(|message| {
            let trimmed = normalize_whitespace(message.content.as_text().as_ref());
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
        prior_envelope
            .map(|envelope| envelope.summary.clone())
            .unwrap_or_else(|| "Session continuity facts preserved.".to_string())
    } else {
        format!("Recent session context: {}", recent.join(" | "))
    }
}

fn merge_grounded_facts(
    prior_envelope: Option<&SessionMemoryEnvelope>,
    original_history: &[Message],
    updates: &[GroundedFactRecord],
) -> Vec<GroundedFactRecord> {
    let mut merged = prior_envelope
        .map(|envelope| envelope.grounded_facts.clone())
        .unwrap_or_default();

    for fact in dedup_latest_facts(original_history, 5) {
        let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
        if let Some(existing_idx) = merged
            .iter()
            .position(|entry| normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized)
        {
            merged.remove(existing_idx);
        }
        merged.push(fact.clone());
    }

    for fact in updates {
        let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
        if let Some(existing_idx) = merged
            .iter()
            .position(|entry| normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized)
        {
            merged.remove(existing_idx);
        }
        merged.push(fact.clone());
    }

    let keep_from = merged.len().saturating_sub(5);
    merged.into_iter().skip(keep_from).collect()
}

fn build_session_memory_envelope(
    session_id: &str,
    workspace_root: &Path,
    original_history: &[Message],
    touched_files: &[String],
    summary: String,
    history_artifact_path: Option<&PathBuf>,
    prior_envelope: Option<&SessionMemoryEnvelope>,
    task_snapshot: &TaskTrackerSnapshot,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> SessionMemoryEnvelope {
    let pe = prior_envelope;
    let spec_summary =
        read_spec_summary(workspace_root).or_else(|| pe.and_then(|e| e.spec_summary.clone()));
    let evaluation_summary = read_evaluation_summary(workspace_root)
        .or_else(|| pe.and_then(|e| e.evaluation_summary.clone()));
    let merge = |prior: &[String], updates: &[String]| {
        merge_recent_strings(prior, updates, MEMORY_LIST_LIMIT)
    };
    let constraints = merge(
        pe.map(|e| e.constraints.as_slice()).unwrap_or(&[]),
        &extract_constraints_from_summary(spec_summary.as_deref()),
    );
    let constraints = merge(
        &constraints,
        &extract_constraints_from_summary(evaluation_summary.as_deref()),
    );
    let update = envelope_update.cloned().unwrap_or_default();

    SessionMemoryEnvelope {
        session_id: session_id.to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary,
        objective: update.objective.or_else(|| {
            task_snapshot
                .objective
                .clone()
                .or_else(|| pe.and_then(|e| e.objective.clone()))
        }),
        task_summary: task_snapshot
            .summary
            .clone()
            .or_else(|| pe.and_then(|e| e.task_summary.clone())),
        spec_summary,
        evaluation_summary,
        constraints: merge(&constraints, &update.constraints),
        grounded_facts: merge_grounded_facts(pe, original_history, &update.grounded_facts),
        touched_files: merge_touched_files(
            pe,
            &touched_files
                .iter()
                .cloned()
                .chain(update.touched_files)
                .collect::<Vec<_>>(),
        ),
        open_questions: merge(
            pe.map(|e| e.open_questions.as_slice()).unwrap_or(&[]),
            &update.open_questions,
        ),
        verification_todo: merge(
            pe.map(|e| e.verification_todo.as_slice()).unwrap_or(&[]),
            &task_snapshot
                .verification_todo
                .iter()
                .cloned()
                .chain(update.verification_todo)
                .collect::<Vec<_>>(),
        ),
        delegation_notes: merge(
            pe.map(|e| e.delegation_notes.as_slice()).unwrap_or(&[]),
            &update.delegation_notes,
        ),
        history_artifact_path: history_artifact_path
            .map(|p| p.display().to_string())
            .or_else(|| pe.and_then(|e| e.history_artifact_path.clone())),
        generated_at: Utc::now().to_rfc3339(),
    }
}

pub(super) fn persist_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
) -> Result<Option<SessionMemoryEnvelope>> {
    let should_persist = should_persist_memory_envelope(vt_cfg);
    if original_history.is_empty()
        || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk)
    {
        return Ok(None);
    }

    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let history_artifact_path =
        if should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk {
            let mut hm = HistoryFileManager::new(workspace_root, session_id);
            let hm2 = messages_to_history_messages(original_history, 0);
            let hr = hm
                .write_history_sync(
                    &hm2,
                    original_history.len(),
                    "compaction",
                    touched_files,
                    &[],
                )
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
        history_artifact_path.as_ref(),
        prior,
        &task_snapshot,
        None,
    );

    if let Some(hap) = history_artifact_path.as_ref() {
        write_memory_envelope_to_path(
            &memory_envelope_path_from_history_path(workspace_root, hap),
            &envelope,
        )?;
    }
    apply_memory_envelope(compacted, &envelope, placement);
    Ok(Some(envelope))
}

pub(crate) fn refresh_session_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    session_stats: &SessionStats,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> Result<Option<SessionMemoryEnvelope>> {
    if history.is_empty() || !should_persist_memory_envelope(vt_cfg) {
        return Ok(None);
    }

    let prior = load_latest_memory_envelope(workspace_root, session_id);
    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let touched_files = session_stats.recent_touched_files();
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        history,
        &touched_files,
        derive_continuity_summary(history, prior.as_ref()),
        None,
        prior.as_ref(),
        &task_snapshot,
        envelope_update,
    );
    let path = latest_memory_envelope_path_for_session(workspace_root, session_id)
        .unwrap_or_else(|| default_memory_envelope_path_for_session(workspace_root, session_id));
    write_memory_envelope_to_path(&path, &envelope)?;
    apply_memory_envelope(history, &envelope, MemoryEnvelopePlacement::Start);
    Ok(Some(envelope))
}
