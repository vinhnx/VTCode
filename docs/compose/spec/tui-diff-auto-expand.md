---
feature: tui-diff-auto-expand
status: delivered
updated: 2026-09-18
branch: feat/tui-diff-auto-expand
commits: 9ad492f8e..6053cdb63+
---

# TUI Diff Auto-Resize and Expand

## Report

**What was built** — Inline TUI transcript diffs no longer ellipsis-truncate ordinary edit/patch body rows: the
tool-output renderer emits logical bodies whole (up to a 2 000-cell safety cap) so transcript reflow can word-wrap them
with a hanging gutter indent. Compact tinted rows that omit the visible gutter hang under the marker cell. Clipped
completed-edit bodies advertise `review full diff for <path>` (vertical omission **and** safety-cap truncation) and
record a UI-only `DiffReviewAnchor`; activating that notice opens full-viewport `ReadonlyReview` via
`DiffOverlayRequest.unified` / `DiffPreviewState::from_unified`. Anchor matching prefers the stored notice, then the
longest specific path, and refuses ambiguous multi-anchor path-less notices. The review overlay stays full-viewport,
keeps `LayoutOptions.wrap = true`, and discloses `+N more` using laid-out wrapped row counts. Overlays do not auto-open
after a completed edit.

### Verification

- `RUSTFLAGS="-D warnings" cargo check -p vtcode-commons -p vtcode-ui -p vtcode --tests --locked` — PASS
- `cargo nextest run -p vtcode-ui` — PASS (1206 tests)
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E "test(tool_output) or test(diff_overlay) or test(safety_capped) or test(review_full) or test(diff_review)"`
  — PASS (221 tests)
- Independent reviews: first pass flagged expand/wrap-blind remaining; re-review confirmed T2–T5 met after safety-cap +
  notice-matching fixes

### Journey log

- The screenshot was transcript truncation, not modal height: `render_diff_content_inline_with_language` ellipsized
  before reflow ever saw the line.
- `vtcode-diff` already had `LayoutOptions.wrap = true`; the overlay problem was leftover truncate-to-width and a
  missing more-rows cue.
- `remaining_inline_rows` must count laid-out wrapped rows; `from_unified` parse failure must yield an empty document,
  not a fabricated after-file body.
- Safety-cap truncation must advertise expand the same way vertical omission does.
- `DiffReviewAnchor.notice` is the activation key; generic path labels (`diff`, `diff.rs`) must never greedily match
  every expand notice.

## [S1] Problem

Transcript-embedded edit/patch diffs ellipsis-truncate long source lines at the measured content width before they reach
the TUI. Vertical budgets omit middle rows behind opaque notices. The review overlay needed wrap-aware remaining-row
disclosure and a path from clipped completed-edit notices to full-viewport review.

## [S2] Design

User-approved decisions:

1. **Transcript wrap** — wrap edit-diff body rows with hanging indent; never ellipsis-truncate when wrapping is possible
   (safety cap 2000 cells).
2. **Overflow strategy** — word wrap is the horizontal overflow policy.
3. **Auto-open policy** — full-viewport review opens for approval/conflict/review prompts, and via explicit expand on a
   clipped completed-edit body. Never auto-modal after a completed edit mid-turn.
4. **Overlay height** — any opened review overlay uses the full viewport.

### S2.1 Transcript path

- Inline TUI sink (`prefers_untruncated_output()`) emits bodies whole up to `DIFF_WRAP_SOURCE_MAX_WIDTH`.
- Compact rows hang under the marker cell when wrapped.
- Vertical omission and safety-cap truncation both advertise `review full diff for <path>` and record
  `DiffReviewAnchor`.

### S2.2 Explicit expand

- `DiffReviewAnchor { file_path, unified, omitted_lines, notice }` in `vtcode_commons::ui_protocol`.
- `InlineCommand::RecordDiffReview` + `AnsiRenderer::record_diff_review`.
- Transcript click on a notice calls `open_diff_review_for_notice` →
  `DiffOverlayRequest { unified, mode: ReadonlyReview }`.
- Matching: stored notice → longest specific path → sole anchor; refuse ambiguous multi-anchor path-less notices.
- No auto-open after completed edits.

### S2.3 Review overlay

- Full viewport; `LayoutOptions.wrap = true`.
- Footer `+N more` uses laid-out wrapped row counts (`remaining_inline_rows` / side-by-side equivalent).
- `DiffPreviewState::from_unified`; parse failure → empty document.

### S2.4–S2.5 Contracts and docs

- `vtcode-diff` renderer-neutral; `ThreadEvent` unchanged.
- Docs: `docs/development/diff-preview.md`, vtcode-ui AGENTS gotcha, README link.

## [S3] Out of Scope

- Tool-registry preview budget changes
- Auto-opening review after completed edits
- Horizontal scroll panes
- Keyboard-only expand (mouse notice activation ships; keybinding polish is follow-up)

## Tasks

- [x] T1: Transcript wrap without ellipsis — acceptance: inline TUI edit-diff rows longer than content width render
      wrapped with hanging gutter indent; no `...` on rows under the safety cap; CLI/sink-absent tests still pass
      (covers: S2.1)
- [x] T2: Compact-row wrap hanging indent — acceptance: gutter-hidden narrow rows wrap with marker-cell indent; no
      unpainted hanging strip (covers: S2.1)
- [x] T3: Expandable omission notice + DiffReviewAnchor — acceptance: clipped completed-edit bodies show an expandable
      notice; activating it opens ReadonlyReview; no auto-modal after edit (covers: S2.1; S2.2)
- [x] T4: DiffPreview unified constructor + overlay expand wiring — acceptance: `DiffOverlayRequest` can open from
      unified content; activation from transcript notice produces full-viewport wrapped review (covers: S2.2; S2.3)
- [x] T5: Overlay wrap + more-rows indicator — acceptance: long lines in overlay are wrapped (not ellipsized) when
      wrap=true; footer discloses remaining rows when content exceeds height (covers: S2.3)
- [x] T6: Regression tests + docs — acceptance: nextest green for touched crates; docs guide + AGENTS gotcha updated if
      needed (covers: S2.1–S2.5; depends: T1–T5)
