# Diff Preview Architecture

VT Code computes text changes in the publishable `vtcode-diff` crate. The crate has no dependency on other VT Code
crates: it owns bounded `similar`-based diff computation, the semantic document model, intraline byte ranges,
Unicode-width wrapping, unified/side-by-side layout, and bounded excerpts.

Application crates own policy and presentation:

- `vtcode-ui` caches the document when an overlay opens and only relayouts or scrolls it during resize and navigation.
- theme and terminal capability selection remain in the VT Code design system; diff styling uses a subtle full-row
  add/delete tint and a stronger background for changed intraline spans. Context rows remain un-tinted, while hunk and
  file metadata use bold syntax-highlighted foregrounds with no background. ANSI16 terminals and explicit no-color
  output fall back to foreground-only styling.
- syntax highlighting is injectable application work, so the reusable crate does not depend on `syntect` or VT Code
  themes.
- approval state, modal priority, `Enter`/`Esc` behavior, and runtime events are unchanged and remain outside the diff
  crate.

`DiffOptions` defaults to practical Myers with separate line and intraline timeouts. Patience and Histogram are public
library choices but are not config values. For large output, preserve readable semantic rows and disable costly
refinement before suppressing content. `LayoutOptions::max_rows` produces a head/tail excerpt with an explicit omission
row.

The user-facing unified layout is still configured as `"inline"` for backward compatibility. Width decisions use the
measured content width after message indentation and transcript framing: side-by-side falls back to unified below the
shared 60-column threshold, and either unified presentation hides its visible `+`/`-`, line-number, and `│` gutter when
keeping it would leave less than 20 columns for source text. The gutter and side-by-side panes return automatically when
a resize restores enough width. Tests should cover asymmetric insert/delete cases, UTF-8 boundaries, display-width
wrapping, deadlines, pairing, exact omission accounting, and both sides of these width boundaries.

## Turn aggregation, event fields, and themes

- `TurnDiffTracker` (`vtcode-core`) aggregates per-file changes within a turn. `get_unified_diff` output is
  deterministic (entries sorted by path) and bounded: entries whose stored content exceeds 200 000 bytes render as a
  one-line summary instead of a full diff.
- The `ThreadEvent::FileChange` payload (`FileChangeItem` in `vtcode-exec-events`) carries optional `unified_diff`,
  `additions`, and `deletions` fields. They are populated from the tracker so consumers can render per-change previews
  without recomputation; older events without the fields deserialize unchanged, and the fields are omitted from output
  when unset.
- `vtcode-diff` skips intraline refinement when any line contains a NUL byte (binary content), preserving line-level
  diffs without paying the word-level cost. CRLF terminators, missing-final-newline hints, and git-compatible
  zero-context hunk headers (`@@ -0,0 +1,N @@`) are covered by regression tests.
- Hunk headers keep their authored range counts. Previews render `@@ -65,19 +65,0 @@` verbatim (and
  `display_lines_from_hunks` synthesizes the same git-compatible form via `format_hunk_header`); collapsing to the
  start-only `@@ -65 +65 @@` is rejected because it misreports a multi-line or pure-deletion hunk as a one-line change.
- Diff syntax highlighting reconstructs each side (pre-image and post-image) in original file order and highlights it
  once, bounded by the existing byte/line caps, so parser state survives hunk boundaries (multi-line strings and
  comments split across hunks stay correctly highlighted). Oversized content falls back to per-hunk highlighting.
- Selectable diff themes live in `vtcode-ui`'s design layer (`DiffTheme`: github, ayu, gruvbox, nord, solarized,
  dracula). Every accent in every theme is contrast-checked against its background at WCAG AA 4.5:1 by
  `every_theme_meets_wcag_aa_contrast`. Use `format_colored_diff_with_theme` for themed rendering; the default theme
  keeps the canonical `format_colored_diff` colors.
- The overlay header truncates the file path so the action label and `(+N -N)` counts remain visible at any width.

## Readability: wrap and expand

User-facing behavior for long diffs in the TUI:

- **Transcript wrap** — when tool output renders into the inline TUI sink, edit/patch body rows are word-wrapped instead
  of ellipsis-truncated, up to a 2 000-cell safety cap (`DIFF_WRAP_SOURCE_MAX_WIDTH` in
  `src/agent/runloop/tool_output/streams.rs`). Continuation rows hang under the visible gutter or, for compact tinted
  rows without a gutter, under the single marker cell. CLI / no-sink renders keep the bounded `MAX_LINE_LENGTH` cap.
- **Vertical omission** — expandable `review full diff` copy and `DiffReviewAnchor` are recorded only when the renderer
  still holds the **complete** unified body and clipped it for display (inline TUI sink, not a pre-truncated registry
  excerpt). Tool-level truncated previews advertise omission/excerpt copy and never claim full-diff review. CLI/no-sink
  keeps the actionable `exec_command`/sed recovery hint.
- **Safety-cap expand** — when an inline-TUI body row exceeds `DIFF_WRAP_SOURCE_MAX_WIDTH`, the transcript still
  ellipsis-truncates that row but also advertises expand (`… diff truncated — review full diff for <path>`) and records
  a `DiffReviewAnchor` with the retained unified payload.
- **Review overlay** — approval/conflict/readonly-review overlays use the full viewport. Layout keeps
  `LayoutOptions.wrap = true`; long lines wrap inside the overlay. When laid-out **wrapped** rows exceed content height
  the controls footer shows `+N more`. Completed-edit review can open from a retained unified preview via
  `DiffOverlayRequest { unified: Some(...), mode: ReadonlyReview }` / `DiffPreviewState::from_unified`.

Spec: `docs/compose/spec/tui-diff-auto-expand.md`.

The presentation architecture was informed by the open-source [Codex](https://github.com/openai/codex) terminal diff
design (Apache-2.0). VT Code's implementation and state model remain independent; no Codex source code was copied.
