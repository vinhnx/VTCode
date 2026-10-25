//! Context-inspector projections derived from the transcript.
//!
//! The inspector renders derived views only: changed files and their parsed
//! diffs come from `FileChangeItem` transcript rows, and the activity
//! timeline comes from tool rows. Nothing here talks to the service.

use std::collections::{HashMap, hash_map::Entry};

use gpui_kit::component::diff::{DiffFile, DiffFileStatus};

use super::transcript::{DiffFileKind, Transcript, TranscriptItem};

/// Cap so a long-running session cannot grow the projection without bound.
const MAX_ACTIVITY_ROWS: usize = 300;

/// Selected inspector view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum InspectorTab {
    #[default]
    Changes,
    Files,
    Activity,
    Output,
    Utilities,
}

impl InspectorTab {
    pub(super) const ALL: [Self; 5] = [
        Self::Changes,
        Self::Files,
        Self::Activity,
        Self::Output,
        Self::Utilities,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Changes => "Changes",
            Self::Files => "Files",
            Self::Activity => "Activity",
            Self::Output => "Output",
            Self::Utilities => "Utilities",
        }
    }
}

/// One file row in the Changes tab, reflecting its newest change-set.
#[derive(Clone, Debug)]
pub(super) struct ChangeEntry {
    pub path: String,
    pub kind: DiffFileKind,
    pub additions: u64,
    pub deletions: u64,
}

/// One row in the Activity tab.
#[derive(Clone, Debug)]
pub(super) struct ActivityRow {
    pub id: String,
    pub title: String,
    pub status: String,
    pub state: ActivityState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActivityState {
    Running,
    Completed,
    Failed,
    Interrupted,
    Other,
}

impl ActivityState {
    pub(super) fn from_status(status: &str) -> Self {
        match status {
            "running" | "in progress" | "started" => Self::Running,
            "completed" => Self::Completed,
            "interrupted" => Self::Interrupted,
            status if status == "failed" || status.starts_with("failed (exit ") => Self::Failed,
            _ => Self::Other,
        }
    }
}

/// Cached projection of every file change-set in the transcript.
#[derive(Default)]
pub(super) struct ChangesProjection {
    /// File rows in newest-change-set-first order.
    entries: Vec<ChangeEntry>,
    /// Parsed diff per path, kept for the review viewer.
    parsed: HashMap<String, ProjectedDiff>,
    /// Totals across the newest state of each file.
    additions: u64,
    deletions: u64,
}

/// The exact transcript item that owns a successfully parsed file preview.
struct ProjectedDiff {
    file: DiffFile,
    source_item_id: String,
}

impl ChangesProjection {
    /// Rebuild from the transcript. Unparseable or missing patches are
    /// skipped; later change-sets overwrite the per-file state.
    pub(super) fn rebuild(transcript: &Transcript) -> Self {
        let mut parsed: HashMap<String, ProjectedDiff> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for item in transcript.items_newest_first() {
            let Some(diff) = item.diff.as_ref() else { continue };
            if diff.patch.trim().is_empty() {
                continue;
            }
            let Ok(files) = DiffFile::parse(&diff.patch) else {
                continue;
            };
            for file in files {
                let path = file.path().to_string();
                if let Entry::Vacant(entry) = parsed.entry(path.clone()) {
                    order.push(path);
                    let _inserted = entry.insert(ProjectedDiff { file, source_item_id: item.id.clone() });
                }
            }
        }
        let mut entries = Vec::with_capacity(order.len());
        let mut additions = 0;
        let mut deletions = 0;
        for path in order {
            let Some(projected) = parsed.get(&path) else { continue };
            let file = &projected.file;
            let file_additions = file.additions() as u64;
            let file_deletions = file.deletions() as u64;
            additions += file_additions;
            deletions += file_deletions;
            entries.push(ChangeEntry {
                path,
                kind: change_kind(file.status()),
                additions: file_additions,
                deletions: file_deletions,
            });
        }
        Self { entries, parsed, additions, deletions }
    }

    pub(super) fn entries(&self) -> &[ChangeEntry] {
        &self.entries
    }

    pub(super) fn totals(&self) -> (u64, u64) {
        (self.additions, self.deletions)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The parsed diff for one file, for the review viewer.
    pub(super) fn select(&self, path: &str) -> Option<DiffFile> {
        self.parsed.get(path).map(|projected| projected.file.clone())
    }

    /// The full raw patch text for the change-set that last touched a path.
    pub(super) fn patch_for(&self, transcript: &Transcript, path: &str) -> Option<String> {
        let source = &self.parsed.get(path)?.source_item_id;
        transcript
            .items_newest_first()
            .find(|item| item.id == *source)?
            .diff
            .as_ref()
            .map(|diff| diff.patch.clone())
    }
}

/// One tool row projected for the Activity tab, with its latest status.
pub(super) fn activity_rows(transcript: &Transcript, restored: &[TranscriptItem]) -> Vec<ActivityRow> {
    let mut rows: Vec<_> = restored
        .iter()
        .chain(transcript.items())
        .rev()
        .filter(|item| item.has_activity_status)
        .take(MAX_ACTIVITY_ROWS)
        .map(|item| {
            let status = item.text.lines().next().unwrap_or_default().to_string();
            ActivityRow {
                id: item.id.clone(),
                title: item.title.clone(),
                state: ActivityState::from_status(&status),
                status,
            }
        })
        .collect();
    rows.reverse();
    rows
}

/// The newest aggregated command/tool output for the Output tab.
pub(super) struct OutputView {
    pub title: String,
    pub text: String,
    pub running: bool,
}

pub(super) fn latest_output(transcript: &Transcript, restored: &[TranscriptItem]) -> Option<OutputView> {
    restored.iter().chain(transcript.items()).rev().find_map(|item| {
        let output = item.output.as_ref()?;
        Some(OutputView {
            title: item.title.clone(),
            text: output.clone(),
            running: item
                .text
                .lines()
                .next()
                .is_some_and(|status| ActivityState::from_status(status) == ActivityState::Running),
        })
    })
}

/// Cap the rendered output tail so a chatty command stays cheap to draw.
pub(super) fn output_tail(text: &str) -> String {
    const MAX_OUTPUT_LINES: usize = 1_000;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= MAX_OUTPUT_LINES {
        return text.to_string();
    }
    let Some(skipped) = lines.len().checked_sub(MAX_OUTPUT_LINES) else {
        return text.to_string();
    };
    let tail = lines.iter().skip(skipped).copied().collect::<Vec<_>>().join("\n");
    format!("… {skipped} earlier lines truncated\n{tail}")
}

fn change_kind(status: DiffFileStatus) -> DiffFileKind {
    match status {
        DiffFileStatus::Added => DiffFileKind::Add,
        DiffFileStatus::Deleted => DiffFileKind::Delete,
        _ => DiffFileKind::Update,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::transcript::{Transcript, TranscriptItem};

    fn diff_item(id: &str, patch: &str) -> TranscriptItem {
        let mut item =
            TranscriptItem::new(id.to_string(), "diff".to_string(), "File changes".to_string(), patch.to_string());
        item.diff = Some(crate::desktop::transcript::DiffCard {
            files: Vec::new(),
            additions: None,
            deletions: None,
            incomplete: false,
            failed: false,
            patch: patch.to_string(),
        });
        item
    }

    #[test]
    fn activity_states_distinguish_failure_interruption_and_unclassified_text() {
        for (status, expected) in [
            ("completed", ActivityState::Completed),
            ("failed", ActivityState::Failed),
            ("failed (exit 17)", ActivityState::Failed),
            ("started", ActivityState::Running),
            ("running", ActivityState::Running),
            ("in progress", ActivityState::Running),
            ("interrupted", ActivityState::Interrupted),
            ("verification requested", ActivityState::Other),
            ("", ActivityState::Other),
        ] {
            assert_eq!(ActivityState::from_status(status), expected, "{status}");
        }
    }

    #[test]
    fn changes_projection_parses_and_orders_files_newest_first() {
        let mut transcript = Transcript::default();
        transcript.push(diff_item(
            "change-1",
            "--- a/src/old.rs\n+++ b/src/old.rs\n@@ -1,2 +1,3 @@\n ctx\n-gone\n+here\n+added\n",
        ));
        transcript.push(diff_item("change-2", "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,2 @@\n ctx\n+new\n"));

        let projection = ChangesProjection::rebuild(&transcript);
        let paths: Vec<&str> = projection.entries().iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs", "src/old.rs"], "newest change-set first");
        assert_eq!(projection.totals(), (3, 1));
        assert!(projection.select("src/main.rs").is_some());
        assert!(projection.select("missing.rs").is_none());
    }

    #[test]
    fn later_change_sets_overwrite_per_file_state() {
        let mut transcript = Transcript::default();
        let older = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,2 +1,3 @@\n-old one\n-old two\n+older one\n+older two\n+older three\n";
        let newer = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,1 +1,2 @@\n keep\n+latest\n";
        assert_eq!(DiffFile::parse(older).unwrap().len(), 1, "the older fixture must be valid");
        transcript.push(diff_item("change-1", older));
        transcript.push(diff_item("change-2", newer));

        let projection = ChangesProjection::rebuild(&transcript);
        assert_eq!(projection.entries().len(), 1, "one row per file");
        assert_eq!(projection.entries()[0].additions, 1);
        assert_eq!(projection.entries()[0].deletions, 0);
        assert_eq!(projection.totals(), (1, 0));
        assert_eq!(projection.patch_for(&transcript, "src/app.rs").as_deref(), Some(newer));
    }

    #[test]
    fn patch_copy_uses_the_parsed_file_owner_not_path_mentions() {
        let mut transcript = Transcript::default();
        let app = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+actual app change\n";
        let backup = "--- a/src/app.rs.bak\n+++ b/src/app.rs.bak\n@@ -1 +1 @@\n-backup\n+mentions src/app.rs\n";
        transcript.push(diff_item("app", app));
        transcript.push(diff_item("backup", backup));
        transcript.push(diff_item("invalid-newer", "src/app.rs is not a patch"));
        let projection = ChangesProjection::rebuild(&transcript);
        assert_eq!(projection.patch_for(&transcript, "src/app.rs").as_deref(), Some(app));
        assert_eq!(projection.patch_for(&transcript, "src/app.rs.bak").as_deref(), Some(backup));
        assert!(projection.patch_for(&transcript, "src/app").is_none());
    }

    #[test]
    fn unparseable_patches_are_skipped() {
        let mut transcript = Transcript::default();
        transcript.push(diff_item("change-bad", "not a diff at all"));
        let projection = ChangesProjection::rebuild(&transcript);
        assert!(projection.is_empty());
    }

    #[test]
    fn activity_rows_project_tool_items_with_status() {
        let mut transcript = Transcript::default();
        transcript.push(TranscriptItem::new(
            "tool-1".to_string(),
            "tool".to_string(),
            "read_file".to_string(),
            "completed\n\n{\"details\": true}".to_string(),
        ));
        transcript.push(TranscriptItem::new(
            "msg-1".to_string(),
            "assistant".to_string(),
            "VT Code".to_string(),
            "not an activity".to_string(),
        ));
        let rows = activity_rows(&transcript, &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "read_file");
        assert_eq!(rows[0].status, "completed");
    }

    #[test]
    fn activity_limit_keeps_the_newest_rows_in_chronological_order() {
        let mut transcript = Transcript::default();
        for index in 0..=MAX_ACTIVITY_ROWS {
            transcript.push(TranscriptItem::new(
                format!("tool-{index}"),
                "tool".into(),
                format!("command {index}"),
                "completed".into(),
            ));
        }
        let rows = activity_rows(&transcript, &[]);
        assert_eq!(rows.len(), MAX_ACTIVITY_ROWS);
        assert_eq!(rows[0].id, "tool-1");
        assert_eq!(rows[MAX_ACTIVITY_ROWS - 1].id, format!("tool-{MAX_ACTIVITY_ROWS}"));
    }

    #[test]
    fn latest_output_tracks_the_newest_streaming_command() {
        let mut transcript = Transcript::default();
        let mut first = TranscriptItem::new(
            "cmd-1".to_string(),
            "tool".to_string(),
            "cargo build".to_string(),
            "completed\n\n{}".to_string(),
        );
        first.output = Some("old output".to_string());
        transcript.push(first);
        let mut second = TranscriptItem::new(
            "cmd-2".to_string(),
            "tool".to_string(),
            "cargo test".to_string(),
            "in progress\n\n{}".to_string(),
        );
        second.output = Some("running output".to_string());
        transcript.push(second);

        let output = latest_output(&transcript, &[]).expect("output projected");
        assert_eq!(output.title, "cargo test");
        assert_eq!(output.text, "running output");
        assert!(output.running, "in-progress status is reflected");
    }

    #[test]
    fn output_tail_truncates_long_streams() {
        let short = "line one\nline two";
        assert_eq!(output_tail(short), short);

        let long = (0..1_050).map(|index| format!("line {index}")).collect::<Vec<_>>().join("\n");
        let tail = output_tail(&long);
        assert!(tail.starts_with("… 50 earlier lines truncated"), "tail notes skipped lines");
        assert!(tail.ends_with("line 1049"));
        assert!(tail.lines().count() <= 1_002);
    }
}
