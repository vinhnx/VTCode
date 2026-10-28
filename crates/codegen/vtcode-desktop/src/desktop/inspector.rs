//! Context-inspector projections derived from the transcript.
//!
//! The inspector renders derived views only: changed files and their parsed
//! diffs come from `FileChangeItem` transcript rows, and the activity
//! timeline comes from tool rows. Nothing here talks to the service.

use std::collections::{HashMap, hash_map::Entry};

use gpui_kit::SharedString;
use gpui_kit::component::diff::{DiffFile, DiffFileStatus};

pub(super) use super::transcript::ActivityState;
use super::transcript::{DiffFileKind, Transcript, TranscriptItem};

/// Cap so a long-running session cannot grow the projection without bound.
pub(super) const MAX_ACTIVITY_ROWS: usize = 300;
const MAX_OUTPUT_LINES: usize = 1_000;
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const OUTPUT_TRUNCATION_NOTICE: &str = "… Earlier output truncated; showing a bounded tail.\n";

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
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub preview_incomplete: bool,
}

/// One row in the Activity tab.
#[derive(Clone, Debug)]
pub(super) struct ActivityRow {
    pub id: String,
    pub title: SharedString,
    pub status: String,
    pub state: ActivityState,
    pub has_output: bool,
}

pub(super) struct ActivityProjection {
    pub rows: Vec<ActivityRow>,
    pub has_older: bool,
}

/// Counts for the bounded activity projection, shared by the utility summary
/// and the Activity tab's chart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ActivityCounts {
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub interrupted: usize,
    pub other: usize,
}

impl ActivityCounts {
    pub(super) fn from_rows(rows: &[ActivityRow]) -> Self {
        let mut counts = Self::default();
        for row in rows {
            match row.state {
                ActivityState::Running => counts.running += 1,
                ActivityState::Completed => counts.completed += 1,
                ActivityState::Failed => counts.failed += 1,
                ActivityState::Interrupted => counts.interrupted += 1,
                ActivityState::Other => counts.other += 1,
            }
        }
        counts
    }

    pub(super) fn total(self) -> usize {
        self.running + self.completed + self.failed + self.interrupted + self.other
    }

    pub(super) fn summary(self) -> String {
        format!(
            "{} running · {} done · {} failed · {} interrupted · {} other",
            self.running, self.completed, self.failed, self.interrupted, self.other
        )
    }
}

/// Cached projection of every file change-set in the transcript.
#[derive(Default)]
pub(super) struct ChangesProjection {
    /// File rows in newest-change-set-first order.
    entries: Vec<ChangeEntry>,
    /// Parsed diff per path, kept for the review viewer.
    projected: HashMap<String, ProjectedChange>,
    /// Totals across the newest state of each file, absent if any file's
    /// preview cannot provide trustworthy counts.
    totals: Option<(u64, u64)>,
}

/// The newest metadata and optional parsed preview for one changed path.
struct ProjectedChange {
    preview: Option<DiffFile>,
    source_item_id: String,
    kind: DiffFileKind,
    additions: Option<u64>,
    deletions: Option<u64>,
    preview_incomplete: bool,
}

impl ChangesProjection {
    /// Rebuild from the transcript. Structured file paths remain visible when
    /// a preview is absent or unparseable; later change-sets overwrite them.
    pub(super) fn rebuild(transcript: &Transcript) -> Self {
        let mut projected: HashMap<String, ProjectedChange> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for item in transcript.items_newest_first() {
            let Some(diff) = item.diff.as_ref() else { continue };
            let mut parsed_files = HashMap::new();
            let mut parsed_order = Vec::new();
            if !diff.patch.trim().is_empty()
                && let Ok(files) = DiffFile::parse(&diff.patch)
            {
                parsed_files.reserve(files.len());
                parsed_order.reserve(files.len());
                for file in files {
                    let path = file.path().to_string();
                    if let Entry::Vacant(entry) = parsed_files.entry(path.clone()) {
                        parsed_order.push(path);
                        let _inserted = entry.insert(file);
                    }
                }
            }

            for file in &diff.files {
                let path = file.path.clone();
                if let Entry::Vacant(entry) = projected.entry(path.clone()) {
                    let preview = parsed_files.remove(&path);
                    let (additions, deletions) = preview_counts(preview.as_ref(), diff.incomplete);
                    order.push(path);
                    let _inserted = entry.insert(ProjectedChange {
                        preview,
                        source_item_id: item.id.clone(),
                        kind: file.kind,
                        additions,
                        deletions,
                        preview_incomplete: diff.incomplete,
                    });
                }
            }

            // Older event versions may omit structured paths. Preserve the
            // parsed-patch fallback, in parser order, for those events.
            for path in parsed_order {
                if let Entry::Vacant(entry) = projected.entry(path.clone()) {
                    let Some(preview) = parsed_files.remove(&path) else {
                        continue;
                    };
                    let (additions, deletions) = preview_counts(Some(&preview), diff.incomplete);
                    order.push(path);
                    let _inserted = entry.insert(ProjectedChange {
                        kind: change_kind(preview.status()),
                        additions,
                        deletions,
                        preview: Some(preview),
                        source_item_id: item.id.clone(),
                        preview_incomplete: diff.incomplete,
                    });
                }
            }
        }
        let mut entries = Vec::with_capacity(order.len());
        let mut totals = Some((0_u64, 0_u64));
        for path in order {
            let Some(change) = projected.get(&path) else { continue };
            totals = totals.and_then(|(additions, deletions)| {
                Some((additions.checked_add(change.additions?)?, deletions.checked_add(change.deletions?)?))
            });
            entries.push(ChangeEntry {
                path,
                kind: change.kind,
                additions: change.additions,
                deletions: change.deletions,
                preview_incomplete: change.preview_incomplete,
            });
        }
        Self { entries, projected, totals }
    }

    pub(super) fn entries(&self) -> &[ChangeEntry] {
        &self.entries
    }

    pub(super) fn totals(&self) -> Option<(u64, u64)> {
        self.totals.or_else(|| self.entries.is_empty().then_some((0, 0)))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The parsed diff for one file, for the review viewer.
    pub(super) fn select(&self, path: &str) -> Option<DiffFile> {
        self.projected.get(path)?.preview.clone()
    }

    pub(super) fn has_preview(&self, path: &str) -> bool {
        self.projected.get(path).is_some_and(|projected| projected.preview.is_some())
    }

    pub(super) fn preview_count(&self) -> usize {
        self.entries.iter().filter(|entry| self.has_preview(&entry.path)).count()
    }

    /// The full raw patch text for the change-set that last touched a path.
    pub(super) fn patch_for(&self, transcript: &Transcript, path: &str) -> Option<String> {
        let projected = self.projected.get(path)?;
        let _preview = projected.preview.as_ref()?;
        let source = &projected.source_item_id;
        transcript
            .items_newest_first()
            .find(|item| item.id == *source)?
            .diff
            .as_ref()
            .map(|diff| diff.patch.clone())
    }
}

/// One tool row projected for the Activity tab, with its latest status.
pub(super) fn activity_rows(transcript: &Transcript, restored: &[TranscriptItem]) -> ActivityProjection {
    let mut activities = restored
        .iter()
        .chain(transcript.items())
        .rev()
        .filter(|item| item.has_activity_status);
    let mut rows: Vec<_> = activities
        .by_ref()
        .take(MAX_ACTIVITY_ROWS)
        .map(|item| {
            let status = item.text.lines().next().unwrap_or_default().to_string();
            ActivityRow {
                id: item.id.clone(),
                title: item.title.clone(),
                state: ActivityState::from_status(&status),
                status,
                has_output: item.output.is_some(),
            }
        })
        .collect();
    let has_older = activities.next().is_some();
    rows.reverse();
    ActivityProjection { rows, has_older }
}

/// The selected or newest aggregated command/tool output for the Output tab.
pub(super) struct OutputView {
    pub id: String,
    pub title: SharedString,
    pub text: SharedString,
    pub running: bool,
}

pub(super) fn output_for(
    transcript: &Transcript,
    restored: &[TranscriptItem],
    selected_id: Option<&str>,
) -> Option<OutputView> {
    selected_id
        .and_then(|id| {
            restored
                .iter()
                .find(|item| item.id == id)
                .or_else(|| transcript.items().find(|item| item.id == id))
        })
        .and_then(output_view)
        .or_else(|| restored.iter().chain(transcript.items()).rev().find_map(output_view))
}

fn output_view(item: &TranscriptItem) -> Option<OutputView> {
    Some(OutputView {
        id: item.id.clone(),
        title: item.title.clone(),
        text: item.output.clone()?,
        running: item
            .text
            .lines()
            .next()
            .is_some_and(|status| ActivityState::from_status(status) == ActivityState::Running),
    })
}

/// Cap the rendered output tail so a chatty command stays cheap to draw.
pub(super) fn output_tail(text: &SharedString) -> SharedString {
    let mut start = text.len().saturating_sub(MAX_OUTPUT_BYTES);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let mut tail = text.get(start..).unwrap_or_default();
    let mut newlines_to_keep = MAX_OUTPUT_LINES + usize::from(tail.ends_with('\n'));
    for (index, byte) in tail.bytes().enumerate().rev() {
        if byte == b'\n' {
            newlines_to_keep -= 1;
            if newlines_to_keep == 0 {
                start += index + 1;
                tail = text.get(start..).unwrap_or_default();
                break;
            }
        }
    }

    if start == 0 {
        return text.clone();
    }

    // `str::lines()` omits one final line terminator; match the previous
    // projection's join behavior when building a truncated tail.
    if let Some(without_terminator) = tail.strip_suffix('\n') {
        tail = without_terminator;
    }
    let mut rendered = String::with_capacity(OUTPUT_TRUNCATION_NOTICE.len().saturating_add(tail.len()));
    rendered.push_str(OUTPUT_TRUNCATION_NOTICE);
    rendered.push_str(tail);
    rendered.into()
}

fn change_kind(status: DiffFileStatus) -> DiffFileKind {
    match status {
        DiffFileStatus::Added => DiffFileKind::Add,
        DiffFileStatus::Deleted => DiffFileKind::Delete,
        _ => DiffFileKind::Update,
    }
}

fn preview_counts(preview: Option<&DiffFile>, incomplete: bool) -> (Option<u64>, Option<u64>) {
    if incomplete {
        return (None, None);
    }
    preview
        .map(|file| (Some(file.additions() as u64), Some(file.deletions() as u64)))
        .unwrap_or((None, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::transcript::{Transcript, TranscriptItem};

    fn diff_item(id: &str, patch: &str) -> TranscriptItem {
        let mut item =
            TranscriptItem::new(id.to_string(), "diff".to_string(), "File changes".into(), patch.to_string());
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
    fn activity_counts_preserve_each_status_category() {
        let rows = [
            ActivityState::Running,
            ActivityState::Completed,
            ActivityState::Completed,
            ActivityState::Failed,
            ActivityState::Interrupted,
            ActivityState::Other,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, state)| ActivityRow {
            id: format!("row-{index}"),
            title: format!("tool {index}").into(),
            status: String::new(),
            state,
            has_output: false,
        })
        .collect::<Vec<_>>();

        let counts = ActivityCounts::from_rows(&rows);
        assert_eq!(counts.total(), rows.len());
        assert_eq!(counts.summary(), "1 running · 2 done · 1 failed · 1 interrupted · 1 other");
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
        assert_eq!(projection.totals(), Some((3, 1)));
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
        assert_eq!(projection.entries()[0].additions, Some(1));
        assert_eq!(projection.entries()[0].deletions, Some(0));
        assert_eq!(projection.totals(), Some((1, 0)));
        assert_eq!(projection.patch_for(&transcript, "src/app.rs").as_deref(), Some(newer));
    }

    #[test]
    fn newest_file_metadata_replaces_stale_preview_when_diff_is_unavailable() {
        let mut transcript = Transcript::default();
        transcript.push(diff_item("older", "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\n"));
        let mut latest =
            TranscriptItem::new("newer".to_string(), "diff".to_string(), "File changes".into(), String::new());
        latest.diff = Some(crate::desktop::transcript::DiffCard {
            files: vec![crate::desktop::transcript::DiffFile {
                path: "src/app.rs".to_string(),
                kind: DiffFileKind::Delete,
            }],
            additions: None,
            deletions: None,
            incomplete: true,
            failed: false,
            patch: String::new(),
        });
        transcript.push(latest);

        let projection = ChangesProjection::rebuild(&transcript);

        assert_eq!(projection.entries().len(), 1, "metadata keeps the changed path visible");
        assert_eq!(projection.entries()[0].path, "src/app.rs");
        assert_eq!(projection.entries()[0].kind, DiffFileKind::Delete);
        assert_eq!(projection.entries()[0].additions, None);
        assert_eq!(projection.entries()[0].deletions, None);
        assert!(projection.entries()[0].preview_incomplete);
        assert!(projection.select("src/app.rs").is_none(), "stale preview is not reused");
        assert!(projection.patch_for(&transcript, "src/app.rs").is_none());
        assert_eq!(projection.totals(), None, "unknown line counts are not reported as zero");
    }

    #[test]
    fn incomplete_preview_remains_openable_without_claiming_partial_counts() {
        let patch = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1,2 @@\n-old\n+new\n+another\n";
        let mut transcript = Transcript::default();
        let mut item = diff_item("partial", patch);
        if let Some(diff) = item.diff.as_mut() {
            diff.incomplete = true;
        }
        transcript.push(item);

        let projection = ChangesProjection::rebuild(&transcript);

        assert!(projection.has_preview("src/app.rs"));
        assert_eq!(projection.entries()[0].additions, None);
        assert_eq!(projection.entries()[0].deletions, None);
        assert!(projection.entries()[0].preview_incomplete);
        assert_eq!(projection.patch_for(&transcript, "src/app.rs").as_deref(), Some(patch));
        assert_eq!(projection.totals(), None);
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
            "read_file".into(),
            "completed\n\n{\"details\": true}".to_string(),
        ));
        transcript.push(TranscriptItem::new(
            "msg-1".to_string(),
            "assistant".to_string(),
            "VT Code".into(),
            "not an activity".to_string(),
        ));
        let rows = activity_rows(&transcript, &[]);
        assert_eq!(rows.rows.len(), 1);
        assert!(!rows.has_older);
        assert_eq!(rows.rows[0].title, "read_file");
        assert_eq!(rows.rows[0].status, "completed");
        assert!(!rows.rows[0].has_output);
    }

    #[test]
    fn activity_limit_keeps_the_newest_rows_in_chronological_order() {
        let mut transcript = Transcript::default();
        for index in 0..=MAX_ACTIVITY_ROWS {
            transcript.push(TranscriptItem::new(
                format!("tool-{index}"),
                "tool".into(),
                format!("command {index}").into(),
                "completed".into(),
            ));
        }
        let rows = activity_rows(&transcript, &[]);
        assert_eq!(rows.rows.len(), MAX_ACTIVITY_ROWS);
        assert!(rows.has_older);
        assert_eq!(rows.rows[0].id, "tool-1");
        assert_eq!(rows.rows[MAX_ACTIVITY_ROWS - 1].id, format!("tool-{MAX_ACTIVITY_ROWS}"));
    }

    #[test]
    fn activity_limit_does_not_report_omitted_rows_at_the_boundary() {
        let mut transcript = Transcript::default();
        for index in 0..MAX_ACTIVITY_ROWS {
            transcript.push(TranscriptItem::new(
                format!("tool-{index}"),
                "tool".into(),
                format!("command {index}").into(),
                "completed".into(),
            ));
        }

        let projection = activity_rows(&transcript, &[]);
        assert_eq!(projection.rows.len(), MAX_ACTIVITY_ROWS);
        assert!(!projection.has_older);
    }

    #[test]
    fn output_defaults_to_the_newest_streaming_command() {
        let mut transcript = Transcript::default();
        let mut first = TranscriptItem::new(
            "cmd-1".to_string(),
            "tool".to_string(),
            "cargo build".into(),
            "completed\n\n{}".to_string(),
        );
        first.output = Some("old output".into());
        transcript.push(first);
        let mut second = TranscriptItem::new(
            "cmd-2".to_string(),
            "tool".to_string(),
            "cargo test".into(),
            "in progress\n\n{}".to_string(),
        );
        second.output = Some("running output".into());
        transcript.push(second);

        let output = output_for(&transcript, &[], None).expect("output projected");
        assert_eq!(output.title, "cargo test");
        assert_eq!(output.text, "running output");
        assert!(output.running, "in-progress status is reflected");
        let rows = activity_rows(&transcript, &[]);
        assert!(rows.rows[0].has_output);
        assert!(rows.rows[1].has_output);
    }

    #[test]
    fn output_selection_recovers_a_specific_historical_capture() {
        let mut transcript = Transcript::default();
        let mut older = TranscriptItem::new(
            "cmd-older".to_string(),
            "tool".to_string(),
            "cargo check".into(),
            "completed".to_string(),
        );
        older.output = Some("older captured output".into());
        transcript.push(older);
        let mut latest = TranscriptItem::new(
            "cmd-latest".to_string(),
            "tool".to_string(),
            "cargo test".into(),
            "completed".to_string(),
        );
        latest.output = Some("latest captured output".into());
        transcript.push(latest);

        let selected = output_for(&transcript, &[], Some("cmd-older")).expect("selected output projected");
        assert_eq!(selected.id, "cmd-older");
        assert_eq!(selected.title, "cargo check");
        assert_eq!(selected.text, "older captured output");

        let latest = output_for(&transcript, &[], Some("missing")).expect("latest output is fallback");
        assert_eq!(latest.id, "cmd-latest");
        assert_eq!(latest.text, "latest captured output");
    }

    #[test]
    fn output_tail_truncates_long_streams() {
        let short: SharedString = "line one\nline two".into();
        assert_eq!(output_tail(&short), short);

        let long: SharedString = (0..1_050)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        let tail = output_tail(&long);
        assert!(tail.starts_with(OUTPUT_TRUNCATION_NOTICE), "tail notes omitted output");
        assert!(tail.ends_with("line 1049"));
        assert_eq!(tail.lines().count(), MAX_OUTPUT_LINES + 1);
    }

    #[test]
    fn output_tail_preserves_a_trailing_newline_at_the_line_limit() {
        let at_limit: SharedString = format!("{}\n", vec!["line"; MAX_OUTPUT_LINES].join("\n")).into();
        assert_eq!(output_tail(&at_limit), at_limit);

        let over_limit: SharedString = format!("{}\n", vec!["line"; MAX_OUTPUT_LINES + 1].join("\n")).into();
        let tail = output_tail(&over_limit);
        assert!(tail.starts_with(OUTPUT_TRUNCATION_NOTICE));
        assert_eq!(tail.lines().count(), MAX_OUTPUT_LINES + 1);
        assert!(tail.ends_with("line"));
    }

    #[test]
    fn output_tail_bounds_long_lines_without_splitting_utf8() {
        let long_line: SharedString = format!("prefix\n{}", "界".repeat(MAX_OUTPUT_BYTES / 3 + 100)).into();
        let tail = output_tail(&long_line);

        assert!(tail.starts_with(OUTPUT_TRUNCATION_NOTICE));
        assert!(tail.len() <= MAX_OUTPUT_BYTES + OUTPUT_TRUNCATION_NOTICE.len());
        assert!(tail.ends_with("界"));
        assert!(tail.is_char_boundary(tail.len()));
    }

    #[test]
    fn output_tail_handles_a_large_single_line_with_a_bounded_tail() {
        let long_line: SharedString = "x".repeat(MAX_OUTPUT_BYTES * 16).into();
        let tail = output_tail(&long_line);

        assert!(tail.starts_with(OUTPUT_TRUNCATION_NOTICE));
        assert_eq!(tail.len(), OUTPUT_TRUNCATION_NOTICE.len() + MAX_OUTPUT_BYTES);
        assert!(tail.ends_with(&"x".repeat(64)));
    }
}
