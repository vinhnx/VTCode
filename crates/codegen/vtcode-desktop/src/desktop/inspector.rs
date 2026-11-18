//! Context-inspector projections derived from the transcript.
//!
//! The inspector renders derived views only: changed files and their parsed
//! diffs come from `FileChangeItem` transcript rows, and the activity
//! timeline comes from tool rows. Nothing here talks to the service.

use std::collections::{HashMap, hash_map::Entry};
use std::rc::Rc;

use gpui_kit::SharedString;
use gpui_kit::component::diff::{DiffFile, DiffFileStatus};

pub(super) use super::transcript::ActivityState;
use super::transcript::{DiffFileKind, Transcript, TranscriptItem};

/// Cap so a long-running session cannot grow the projection without bound.
pub(super) const MAX_ACTIVITY_ROWS: usize = 300;
#[cfg(test)]
use super::output_tail::{MAX_OUTPUT_BYTES, MAX_OUTPUT_LINES};
pub(super) use super::output_tail::{compact_output_tail, display_output_tail, output_tail};

/// Selected inspector view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum InspectorTab {
    #[default]
    Changes,
    Files,
    Activity,
    Tasks,
    Output,
    Utilities,
    Plan,
}

impl InspectorTab {
    pub(super) const ALL: [Self; 7] = [
        Self::Changes,
        Self::Files,
        Self::Activity,
        Self::Tasks,
        Self::Output,
        Self::Utilities,
        Self::Plan,
    ];

    pub(super) fn label(self) -> String {
        match self {
            Self::Changes => rust_i18n::t!("app.inspector.tab_changes").to_string(),
            Self::Files => rust_i18n::t!("app.inspector.tab_files").to_string(),
            Self::Activity => rust_i18n::t!("app.inspector.tab_activity").to_string(),
            // English literal: no translated key covers the Tasks tab yet.
            Self::Tasks => "Tasks".to_string(),
            Self::Output => rust_i18n::t!("app.inspector.tab_output").to_string(),
            Self::Utilities => rust_i18n::t!("app.inspector.tab_utilities").to_string(),
            Self::Plan => rust_i18n::t!("app.inspector.tab_plan").to_string(),
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
    pub edit_path: Option<SharedString>,
    /// Whether the row resolves in the live transcript. Restored-only rows
    /// predate the loaded window, so they offer output but no transcript jump.
    pub in_transcript: bool,
}

#[derive(Clone, Debug)]
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
        rust_i18n::t!(
            "app.inspector.activity_summary",
            running = self.running,
            done = self.completed,
            failed = self.failed,
            interrupted = self.interrupted,
            other = self.other
        )
        .to_string()
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

    /// Previewable entries matching an optional kind filter.
    pub(super) fn filtered_preview_count(&self, filter: Option<DiffFileKind>) -> usize {
        self.entries
            .iter()
            .filter(|entry| filter.is_none_or(|kind| kind == entry.kind))
            .filter(|entry| self.has_preview(&entry.path))
            .count()
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
///
/// Rows read newest-first so the live edge stays visible without scrolling;
/// the cap keeps the newest rows and reports the dropped tail via `has_older`.
pub(super) fn activity_rows(transcript: &Transcript, restored: &[TranscriptItem]) -> ActivityProjection {
    let mut activities = restored
        .iter()
        .map(|item| (item, false))
        .chain(transcript.items().map(|item| (item, true)))
        .rev()
        .filter(|(item, _)| item.has_activity_status);
    let rows: Vec<_> = activities
        .by_ref()
        .take(MAX_ACTIVITY_ROWS)
        .map(|(item, in_transcript)| {
            let status = item.text.lines().next().unwrap_or_default().to_string();
            ActivityRow {
                id: item.id.clone(),
                title: item.title.clone(),
                state: ActivityState::from_status(&status),
                status,
                has_output: item.has_output(),
                edit_path: item.edit_path.clone(),
                in_transcript,
            }
        })
        .collect();
    let has_older = activities.next().is_some();
    ActivityProjection { rows, has_older }
}

/// The selected or newest aggregated command/tool output for the Output tab.
#[derive(Clone, Debug)]
pub(super) struct OutputView {
    pub id: String,
    pub row_id: String,
    pub title: SharedString,
    pub text: SharedString,
    pub running: bool,
    pub truncated: bool,
}

pub(super) fn output_for(
    transcript: &Transcript,
    restored: &[TranscriptItem],
    selected_id: Option<&str>,
) -> Option<OutputView> {
    if let Some(item) = selected_id.and_then(|id| {
        restored
            .iter()
            .chain(transcript.items())
            .find(|item| item.id == id || item.output_item_id.as_deref() == Some(id))
    }) {
        let selected = output_view(item).or_else(|| {
            item.has_archived_output().then(|| OutputView {
                id: item.id.clone(),
                row_id: item.id.clone(),
                title: item.title.clone(),
                text: super::output_sections::human_output_text(&item.text).into(),
                truncated: false,
                running: false,
            })
        });
        if selected.is_some() || item.session_poll.is_some() {
            return selected;
        }
    }
    restored.iter().chain(transcript.items()).rev().find_map(output_view)
}

fn output_view(item: &TranscriptItem) -> Option<OutputView> {
    Some(OutputView {
        id: item.output_item_id.clone().unwrap_or_else(|| item.id.clone()),
        row_id: item.id.clone(),
        title: item.title.clone(),
        text: output_with_truncation_notice(item.output.as_ref()?, item.output_truncated),
        truncated: item.output_truncated,
        running: item
            .text
            .lines()
            .next()
            .is_some_and(|status| ActivityState::from_status(status) == ActivityState::Running),
    })
}

pub(super) fn output_with_truncation_notice(text: &SharedString, truncated: bool) -> SharedString {
    let notice = rust_i18n::t!("app.inspector.truncation_notice");
    if truncated && !text.starts_with(notice.as_ref()) {
        format!("{notice}{text}").into()
    } else {
        text.clone()
    }
}

/// Narrow output text to lines matching a case-insensitive substring query.
///
/// Returns the kept text plus matched/total line counts. An empty query keeps
/// everything, so callers only branch on whether the query is blank.
pub(super) fn filter_output_lines(tail: &str, query: &str) -> (String, usize, usize) {
    let total = tail.lines().count();
    let query = query.trim();
    if query.is_empty() {
        return (tail.to_string(), total, total);
    }
    let folded = query.to_lowercase();
    let mut kept = String::new();
    let mut matched = 0;
    for line in tail.lines() {
        if line.to_lowercase().contains(&folded) {
            if matched > 0 {
                kept.push('\n');
            }
            kept.push_str(line);
            matched += 1;
        }
    }
    (kept, matched, total)
}

/// Memoized [`activity_rows`] projection, keyed on both input generations.
///
/// The activity timeline derives from the live transcript plus restored
/// history rows; a frame reuses the shared projection when neither input
/// changed since the last render. `Rc` keeps cache hits allocation-free.
#[derive(Clone, Debug)]
pub(super) struct ActivityProjectionCache {
    activity_revision: u64,
    restored_revision: u64,
    projection: Rc<ActivityProjection>,
}

impl ActivityProjectionCache {
    pub(super) fn fresh(activity_revision: u64, restored_revision: u64, projection: ActivityProjection) -> Self {
        Self {
            activity_revision,
            restored_revision,
            projection: Rc::new(projection),
        }
    }

    /// Shared projection when both inputs are unchanged since `fresh`.
    pub(super) fn get(&self, activity_revision: u64, restored_revision: u64) -> Option<Rc<ActivityProjection>> {
        (self.activity_revision == activity_revision && self.restored_revision == restored_revision)
            .then(|| self.shared())
    }

    /// Shared projection without a freshness check, for the frame that built it.
    pub(super) fn shared(&self) -> Rc<ActivityProjection> {
        Rc::clone(&self.projection)
    }
}

/// Memoized [`output_for`] + [`output_tail`] pair, keyed on both input
/// generations plus the selected output id (the fallback newest-output
/// resolution also depends on the selection).
#[derive(Clone, Debug)]
pub(super) struct OutputViewCache {
    transcript_revision: u64,
    restored_revision: u64,
    selected_id: Option<String>,
    /// Cached resolution; `None` means "no output" was the fresh answer.
    view: Option<Rc<OutputView>>,
    /// Bounded tail of `view.text`; empty when `view` is `None`.
    tail: SharedString,
}

impl OutputViewCache {
    pub(super) fn fresh(
        transcript_revision: u64,
        restored_revision: u64,
        selected_id: Option<String>,
        view: Option<OutputView>,
    ) -> Self {
        let tail = view
            .as_ref()
            .map_or_else(SharedString::default, |view| display_output_tail(&view.text));
        Self {
            transcript_revision,
            restored_revision,
            selected_id,
            view: view.map(Rc::new),
            tail,
        }
    }

    /// Shared resolution plus its bounded tail when nothing relevant changed.
    pub(super) fn get(
        &self,
        transcript_revision: u64,
        restored_revision: u64,
        selected_id: Option<&str>,
    ) -> Option<(Option<Rc<OutputView>>, SharedString)> {
        (self.transcript_revision == transcript_revision
            && self.restored_revision == restored_revision
            && self.selected_id.as_deref() == selected_id)
            .then(|| self.shared())
    }

    /// Shared resolution plus its bounded tail, for the frame that built it.
    pub(super) fn shared(&self) -> (Option<Rc<OutputView>>, SharedString) {
        (self.view.clone(), self.tail.clone())
    }
}

/// One memoized [`output_tail`] for an expanded transcript tool row.
///
/// Stored in a row-id-keyed map on the view; the entry is valid only at
/// `transcript_revision`, and the resolver prunes stale entries on every
/// miss so the map only ever holds current-revision tails.
#[derive(Clone, Debug)]
pub(super) struct ToolTailCacheEntry {
    pub(super) transcript_revision: u64,
    pub(super) tail: SharedString,
}

/// Memoized [`output_tail`] for one expanded transcript tool row. Pure in
/// its inputs so unit tests can pin the hit/miss/prune contract without
/// a window; entries from older revisions are pruned on every miss so
/// the map only holds tails for currently expanded rows.
pub(super) fn resolve_tool_tail(
    id: &str,
    output: &SharedString,
    revision: u64,
    cache: &mut HashMap<String, ToolTailCacheEntry>,
) -> SharedString {
    if let Some(entry) = cache.get(id)
        && entry.transcript_revision == revision
    {
        return entry.tail.clone();
    }
    let tail = compact_output_tail(output);
    cache.retain(|_, entry| entry.transcript_revision == revision);
    let _previous =
        cache.insert(id.to_string(), ToolTailCacheEntry { transcript_revision: revision, tail: tail.clone() });
    tail
}

fn change_kind(status: DiffFileStatus) -> DiffFileKind {
    match status {
        DiffFileStatus::Added => DiffFileKind::Add,
        DiffFileStatus::Deleted => DiffFileKind::Delete,
        _ => DiffFileKind::Update,
    }
}

pub(super) fn preview_counts(preview: Option<&DiffFile>, incomplete: bool) -> (Option<u64>, Option<u64>) {
    if incomplete {
        return (None, None);
    }
    preview
        .map(|file| (Some(file.additions() as u64), Some(file.deletions() as u64)))
        .unwrap_or((None, None))
}
#[cfg(test)]
#[path = "inspector_tests.rs"]
mod tests;
