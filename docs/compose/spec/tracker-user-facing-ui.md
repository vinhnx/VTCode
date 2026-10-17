---
feature: tracker-user-facing-ui
status: delivered
updated: 2026-09-18
branch: feat/tracker-user-facing-ui
commits: 9c4a68045..63c85b924
---

# Tracker User-Facing UI (Title + Progress Only)

## Report

**What was built** — User-facing task tracking now answers “what is this work, and how far along is it?” without dumping
internal checklists. Three surfaces share one contract:

1. **Transcript** (`tracker_progress_lines`): successful `task_tracker` updates collapse to one line — `• Release 3/8`
   (humanized title + counts; no `next:` snippet, no step tree). Empty/failed trackers keep diagnostic lines. Non-inline
   `render_tracker_view` follows the same contract.
2. **Docked panel** (`tracker_tree_body_lines` + typed header): Alt+G body keeps the compact tree when opened; header
   shows title + `N/M complete`. Visibility-only show/hide requests never mutate body/metadata/progress.
3. **Terminal title**: progress comes only from typed `TaskPanelMetadata` (`N/M`). Content updates own that state —
   `Some(metadata)` sets progress; `metadata: None` (session clear) clears it. Line-parse `Progress:` extraction is
   removed.

Internal `task_tracker` payloads, structured `files`/`outcome`/`verify` metadata, and continuation auto-queue behavior
are unchanged. `tracker_panel_metadata` intentionally derives counts from checklist items when explicit totals are
missing and falls back to title `"Task tracker"`.

**Verification** — commands run and observed results:

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-ui -E 'test(task_panel) or test(terminal_title) or test(visibility_requests) or test(alt_g) or test(metadata_progress)'`
  — PASS (30)
- `cargo nextest run -p vtcode -E 'test(tracker_) or test(task_progress)'` — PASS (70)
- `./scripts/check-dev.sh` — PASS (fmt, clippy `-D warnings`, compile, shell lint)
- Independent reviews: first pass flagged `show_task_panel()` body wipe + dual progress writers; re-review confirmed
  those fixed but found content-clear left stale metadata/progress; third pass confirmed the clear-path fix and T1–T5
  acceptance.

**Journey log** —

1. Prior `tracker-continuation` work on main already auto-continued incomplete TODO steps; this follow-up owns only
   presentation.
2. Transcript and panel previously shared `tracker_view_lines` (full compact tree + `next:`). Split into progress vs
   tree helpers; production `tracker_view_lines` and next-snippet helpers were removed.
3. First review: `show_task_panel()` sent empty lines and the handler always overwrote body; terminal title dual-sourced
   from line parse + metadata.
4. Visibility-only requests are now non-destructive; metadata is the sole progress writer. Second review found `/clear`
   (`update_task_panel(Vec::new())`) no longer reset progress after the parse path was deleted — content updates without
   metadata now clear metadata + progress.
5. Recoverable lesson: removing an accidental reset path requires an explicit clear on the intentional reset path;
   visibility mutations and content mutations must stay separate ownership rules.

## [S1] Problem

The product goal for user-facing task tracking is: show only the relevant task **title and progress**, hide internal
tracking details, and keep the internal tracker robust for the agent and session resume.

Before this change every successful `task_tracker` update painted the **full compact tree** into both the transcript
(`• Tasks 1/4 — next: …` plus every step row) and the docked task-panel body. Operational step lists, status glyphs, and
next-step snippets are harness detail; the user surface should answer “what is this work, and how far along is it?”
without dumping the checklist on every update.

Internal tracking was already complete: `[agent.harness.continuation]` auto-continues incomplete tracker work across
turns/resume, and the tool payload still carries full checklist items plus structured `files`/`outcome`/`verify`
metadata. The gap was presentation only.

## [S2] Design

Decision (user-confirmed):

- **User-facing surface = title + progress only.**
- Transcript tracker updates are a single progress line.
- Docked task panel (Alt+G) keeps the compact tree in its **body when the user opens it**; header already shows title +
  progress.
- Internal `task_tracker` payload and continuation behavior stay unchanged.

### Contracts

**View split** in `src/agent/runloop/tool_output/mod.rs` (binary presentation layer):

```rust
/// Transcript-facing user surface: title + progress only (plus diagnostics).
pub(crate) fn tracker_progress_lines(val: &serde_json::Value) -> Vec<String>

/// Panel body: compact tree rows only (no summary header / no next: snippet).
pub(crate) fn tracker_tree_body_lines(val: &serde_json::Value) -> Vec<String>
```

- `tracker_progress_lines` success shape (one line):
  - `• {humanized_title} {completed}/{total}` when progress counts exist (explicit or derived)
  - `• Tasks {completed}/{total}` otherwise when counts exist without a title
  - **No** `— next:` snippet, **no** tree rows
  - Emits the progress line even when the tree body is empty if counts exist
- `tracker_progress_lines` failure/empty/malformed: keep existing diagnostic lines (`Tracker status: …`, `Update: …`) so
  errors stay visible
- `tracker_tree_body_lines`: compact tree rows only; strip `files:` / `outcome:` / `verify:` as today; do not prepend
  the summary header
- `tracker_panel_metadata(val)`: humanized title (fallback `"Task tracker"`) + counts via `tracker_progress_counts`
  (explicit wins; else derive from renderable items)
- Structured tool payload for the model is **not** narrowed

#### Call-site wiring

1. `src/agent/runloop/unified/tool_output_handler.rs` (`is_task_tracker_tool` path):
   - Panel: `update_task_panel_with_metadata(tracker_tree_body_lines(output), tracker_panel_metadata(output))`
   - Transcript: `apply_task_tracker_block(..., tracker_progress_lines(output))`
2. `src/agent/runloop/unified/planning_workflow/task_tracker.rs` (`render_created_task_tracker`):
   - Same split; approval handoff may still `show_task_panel()` once
   - Transcript append / replace uses progress-only lines (dedupe on those lines)
3. Terminal title (`vtcode-ui` session):
   - Typed `TaskPanelMetadata` is the **only** writer of `terminal_title_task_progress` (`completed/total`)
   - Visibility-only TaskPanel requests (`show_task_panel` / `hide_task_panel`) must **not** mutate body lines,
     metadata, or progress
   - Content updates **own** metadata state: `Some(metadata)` sets `N/M`; `None` (session clear) clears metadata and
     progress
   - Line-parse `Progress:` extraction is removed (panel body has no such row)

**Panel UI** (`vtcode-ui` task panel)

- Header: humanized title + `{completed}/{total} complete`
- Body: compact tree when panel is visible; empty-state constant when no rows
- Appearance-suppressed panel still retains lines for later manual toggle

#### Prompt / docs surface

- Binary gotcha no longer requires transcript + panel to share `tracker_view_lines` as one compact tree
- User-facing docs describe title+progress transcript vs panel tree

### Config

No new config keys. Presentation is unconditional under the existing tracker UI; kill-switches for continuation remain
`[agent.harness.continuation]`.

### Intentional contract clarifications

- `tracker_panel_metadata` falls back to title `"Task tracker"` when checklist title is missing and derives
  `completed`/`total` via `tracker_progress_counts` (explicit counts win; otherwise count renderable items). Required
  for terminal-title progress without explicit count fields.
- Successful checklists with progress counts but empty tree body still emit the one-line progress surface in the
  transcript.

## [S3] Out of Scope

- Changing `task_tracker` tool schema, persistence, or model-facing payload shape
- Changing continuation / resume auto-queue behavior (`tracker-continuation` stays as delivered)
- Auto-opening the task panel on every tracker update
- Redesigning status glyphs or panel chrome beyond the progress/title vs tree split
- Fixing unrelated TODO.md owner notes

## Tasks

- [x] T1: Add `tracker_progress_lines` + `tracker_tree_body_lines` with unit tests — acceptance: success emits one
      title+progress line (no next/tree); errors keep diagnostics; tree body has no summary header and no metadata rows
      (covers: S2)
- [x] T2: Wire tool_output_handler + planning approval handoff to the view split — acceptance: transcript replace uses
      progress-only; panel receives tree body + metadata; existing dedupe still holds (covers: S2; depends: T1)
- [x] T3: Terminal title progress from panel metadata only — acceptance: metadata update sets `N/M`; visibility-only
      show/hide does not wipe body/metadata/progress; content updates without metadata clear progress (covers: S2;
      depends: T2)
- [x] T4: Update transcript/panel tests that assert full-tree user-facing blocks — acceptance: tests assert
      progress-only transcript rows and tree rows only on the panel path (covers: S2; depends: T2)
- [x] T5: Update AGENTS/gotcha/docs presentation contract — acceptance: binary gotcha no longer mandates shared
      full-tree transcript rendering; docs describe title+progress transcript vs panel tree (covers: S2; depends: T2)
