use super::*;

/// Short single-line sidebar title derived from a session's first prompt,
/// mirroring the archive's first-prompt preview.
pub(in super::super) fn session_title_from_prompt(prompt: &str) -> String {
    const MAX_TITLE_CHARS: usize = 72;
    let first_line = prompt.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    let trimmed = first_line.trim();
    let mut title: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    if trimmed.chars().count() > MAX_TITLE_CHARS {
        title.push('…');
    }
    title
}

pub(in super::super) fn restored_history_element_id(
    session_id: &str,
    role: &str,
    text: &str,
    occurrence: usize,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut hasher);
    role.hash(&mut hasher);
    text.hash(&mut hasher);
    occurrence.hash(&mut hasher);
    format!("history-{:016x}", hasher.finish())
}

fn restored_archive_element_id(session_id: &str, archive_index: usize) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut hasher);
    format!("history-{:016x}-{archive_index}", hasher.finish())
}

/// Prepare stable transcript rows and Markdown accessibility projections for archived messages.
pub(in super::super) fn prepare_session_history(
    session_id: &str,
    messages: Vec<SessionHistoryMessage>,
) -> Vec<TranscriptItem> {
    let mut occurrences = HashMap::<u64, usize>::new();
    let mut phases = Vec::new();
    let rows: Vec<TranscriptItem> = messages
        .into_iter()
        .filter(|message| !message.hidden || message.role != TranscriptRole::System.as_str())
        .map(|message| {
            let SessionHistoryMessage {
                archive_index,
                role,
                mut text,
                phase,
                event_item_id,
                ..
            } = message;
            phases.push(phase.clone());
            let title = match TranscriptRole::from_str(&role) {
                Some(TranscriptRole::User) => rust_i18n::t!(i18n::TRANSCRIPT_YOU).to_string(),
                Some(TranscriptRole::Assistant) => app::NAME.to_string(),
                Some(TranscriptRole::Tool) => rust_i18n::t!("app.transcript.tool_result").to_string(),
                _ => role.clone(),
            };
            let id = archive_index.map_or_else(
                || {
                    let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
                    role.hash(&mut fingerprint);
                    text.hash(&mut fingerprint);
                    let occurrence = occurrences.entry(fingerprint.finish()).or_default();
                    let id = restored_history_element_id(session_id, &role, &text, *occurrence);
                    *occurrence = (*occurrence).saturating_add(1);
                    id
                },
                |index| restored_archive_element_id(session_id, index),
            );
            if role == TranscriptRole::User.as_str() {
                text = strip_file_reference_metadata(&text).to_owned();
            } else if role == TranscriptRole::Tool.as_str() {
                text = human_output_text(&text);
            }
            // Preserve collapsed archive tool text without inventing activity status.
            let mut item = TranscriptItem::new(id, role, title.into(), text);
            item.archived_phase = phase.map(Into::into);
            item.canonical_item_id = event_item_id.filter(|id| !id.is_empty() && id.len() <= 512).map(Into::into);
            item.has_activity_status = false;
            item.prepare_render_content();
            item
        })
        .collect();
    Transcript::group_archive_turns(rows, phases)
}

/// Hide the generated file-reference suffix while retaining user-authored text.
fn strip_file_reference_metadata(text: &str) -> &str {
    const MARKER: &str = "\n\n[file_reference_metadata]\n";
    const HINT: &str =
        "Hint: Read each referenced file once using the resolved path above. Do not re-read unless truncated.";
    let Some((body, metadata)) = text.rsplit_once(MARKER) else {
        return text;
    };
    let Some(lines) = metadata.trim_end().strip_suffix(HINT) else {
        return text;
    };
    let mut mappings = lines.lines().filter(|line| !line.is_empty());
    if mappings.clone().next().is_some() && mappings.all(|line| line.starts_with('@') && line.contains('=')) {
        body
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::{prepare_session_history, restored_history_element_id, strip_file_reference_metadata};
    use vtcode_desktop_protocol::SessionHistoryMessage;

    #[test]
    fn removes_only_a_complete_generated_file_reference_suffix() {
        let text = "Fix @README.md\n\n[file_reference_metadata]\n@README.md=/work/README.md\nHint: Read each referenced file once using the resolved path above. Do not re-read unless truncated.\n";
        assert_eq!(strip_file_reference_metadata(text), "Fix @README.md");
        for text in [
            "Discuss [file_reference_metadata]",
            "example\n\n[file_reference_metadata]\nmanual notes",
            "@README.md\n\n[file_reference_metadata]\n@README.md=/work/README.md\n",
        ] {
            assert_eq!(strip_file_reference_metadata(text), text);
        }
    }

    #[test]
    fn restored_projection_keeps_raw_identity_and_only_cleans_user_text() {
        let raw = "Fix @README.md\n\n[file_reference_metadata]\n@README.md=/work/README.md\nHint: Read each referenced file once using the resolved path above. Do not re-read unless truncated.\n";
        let message = |role: &str| SessionHistoryMessage {
            hidden: false,
            archive_index: None,
            role: role.into(),
            phase: None,
            event_item_id: None,
            text: raw.into(),
        };
        let rows = prepare_session_history("s", vec![message("user"), message("assistant")]);
        assert_eq!(rows[0].text, "Fix @README.md");
        assert_eq!(rows[0].id, restored_history_element_id("s", "user", raw, 0));
        assert_eq!(rows[1].text, raw);
    }
}
