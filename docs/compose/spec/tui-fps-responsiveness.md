---
feature: tui-fps-responsiveness
status: delivered
updated: 2026-09-28
branch: perf/tui-fps-responsiveness
commits: 998ca2a2d..29c500dd1
---

# TUI FPS and Responsiveness

## Report

**What was built** — Steady-state TUI jank is now measurable and the main hot-path thrash is gone. An opt-in frame
sampler (`VTCODE_TUI_FRAME_METRICS=1`, `tui/frame_metrics.rs`) records draw and input-to-draw durations into a
256-sample ring and logs p50/p95/max plus slow counts every 5s on `vtcode.tui.latency`; the flag is a no-op when unset.
Streaming appends and `push_line` use `mark_transcript_line_dirty` so header/sidebar caches survive every chunk.
Transcript eviction drops only the reflow-cache prefix (`TranscriptReflowCache::evict_prefix`) instead of wiping up to
5000 cached messages. The transcript paint path precomputes row tints and moves lines into `Paragraph` (no second
clone); header lines are shared via `Arc` through measure → layout → widget. Hover/scroll use `mark_visual_dirty`.
Capture retention is bounded (`TUI_TOOL_OUTPUT_CAPTURE_MAX_LINES`, `TUI_TOOL_OUTPUT_BLOCKS_MAX`,
`TUI_COMPACT_ACTIVITY_MAX_ENTRIES`, `TUI_COLLAPSED_PASTE_MAX_BYTES`).

**Verification** — `cargo nextest run -p vtcode-ui --lib` PASS 1350/1350 (includes new regression tests for header-cache
preservation, partial eviction, capture/paste bounds, frame-budget smoke).
`cargo clippy -p vtcode-ui --locked -- -D warnings` PASS. `cargo fmt -p vtcode-ui -- --check` PASS.
`large_transcript_render_stays_under_frame_budget`: 200 frames of an 800-line transcript on TestBackend 120×40 averaged
**~170 µs/frame** (well under the 16 ms active-tick budget). Independent re-review confirmed the four critical findings
resolved; residual notes (1–3-line header `Vec` clone into `HeaderWidget`, wall-clock assert loosened to 50 ms for CI)
are non-blocking.

**Journey log** — Browser Use/IAB was unavailable (`js`/`cua_repl` missing), so ratatui/crossterm research used pinned
local crate sources plus project gotchas instead of open-web pages. First review caught that `mark_line_dirty` was still
thrashing header caches on stream append and that eviction did a full cache wipe — both were the highest-leverage jank
sources. Header “built once” needed `Arc` through the whole measure/layout/widget chain, not just a cache flag.
`frame_metrics` percentile indexing used a floor formula that under-reported p99 on small rings; nearest-rank fixed it.
Follow-up audit (29c500dd1): FIFO capture trim no longer drops the newest block or an open review’s captures (re-trim on
viewer close); eviction no longer invents `first_dirty_line=0`.

## [S1] Problem

Under heavy load (streaming tool/PTY output, long transcripts, rapid input, large diffs) the TUI can drop frames and lag
input. Users report jank. There is no steady-state frame metric — only `vtcode.tui.latency` debug logs at 16ms/8ms
thresholds — so jank cannot be measured before/after. Prior allocation work reduced some hot-path waste, but several
per-frame clone/alloc patterns and cache-invalidation thrash remain.

Root causes identified in the render/input pipeline (file:line on `perf/tui-fps-responsiveness` base `998ca2a2d`):

1. **Per-frame visible-window clones** — `get_visible_range` clones every visible `TranscriptLine`
   (`session/transcript.rs`); `decorate_*_links` clones `line.line` even with zero links (`transcript_links.rs`);
   `Paragraph::new(visible_lines.clone())` cloned again (`widgets/transcript.rs`). Header lines cloned twice.
2. **Stream-append invalidation thrash** — every `append_text`/`push_line` called `mark_line_dirty` → `mark_dirty()`,
   dropping header + preview caches per streamed chunk. Eviction/theme retint called `invalidate_transcript_cache` →
   full 5000-message reflow.
3. **Per-cell String alloc in hit-region rebuild** — `find_expand_action_text_region` allocated `expected.to_string()`
   per cell×char (`app/session/impl_render.rs`).
4. **Dirty-flag overkill on input** — mouse hover and scroll called `mark_dirty()` instead of `mark_visual_dirty`.
5. **Unbounded retention** — full PTY captures in `ToolOutputBlock.lines`, unbounded `compact_activity_entries`, full
   paste text in `collapsed_pastes`.

## [S2] Design

### Goals

- Sustained interactive redraw without dropped frames under streaming load, long transcripts, and large diffs.
- Keypress-to-draw stays on the same wakeup (already true — preserve).
- Measurable before/after: lean frame metrics under an opt-in flag.
- No unbounded TUI-side retention for tool/PTY captures and activity lists.

### Frame metrics (lean)

`vtcode-ui` `tui/frame_metrics.rs`, opt-in via `VTCODE_TUI_FRAME_METRICS=1`:

- Counters: frames drawn, frames skipped, slow draws (≥8ms), slow input-to-draw (≥16ms), maxima in window.
- Rolling window: last 256 draw durations (ring) for p50/p95; report via `vtcode.tui.latency` every 5s when enabled.
- Wired from `render_if_dirty` only; zero cost when flag is off.
- No in-TUI overlay (user prioritized FPS fix over observability surface).

### Render-path fixes

1. **Cut clones**: `Paragraph` consumes lines after precomputed row tints; header lines shared as `Arc<Vec<Line>>`
   through measure → layout → widget (built once per dirty).
2. **Stream-append dirty discipline**: `append_text`/`push_line` use `mark_transcript_line_dirty`; eviction uses
   `TranscriptReflowCache::evict_prefix` (surviving entries stay valid).
3. **Hit-region alloc**: stack UTF-8 compare, no per-cell `String`.
4. **Input dirty flags**: hover/scroll use `mark_visual_dirty`.

### Memory bounds

`TUI_TOOL_OUTPUT_CAPTURE_MAX_LINES` (tail), `TUI_TOOL_OUTPUT_BLOCKS_MAX` (FIFO), `TUI_COMPACT_ACTIVITY_MAX_ENTRIES`
(FIFO), `TUI_COLLAPSED_PASTE_MAX_BYTES` (tail). Constants in `ui.rs`, documented in `docs/development/performance.md`.

### ratatui / crossterm practices (local sources; open-web research unavailable)

Pinned ratatui `=0.30.2` (`layout-cache` on), crossterm 0.29 fork. Preserve dirty-flag redraw, adaptive tick, biased
input select, event/scroll coalescing, viewport clamps, and allocation-conscious caches (gotcha 2026-07-20).

### Contracts

- `frame_metrics` is `pub(crate)` inside `vtcode-ui`; no new `ThreadEvent`, no config schema change.
- Public TUI behavior unchanged except performance and bounded retention.
- Regression tests cover append-preserves-header-cache, partial eviction, bounds, and frame-budget smoke.

## [S3] Out of Scope

- In-TUI FPS overlay / HUD.
- New config keys or TOML schema.
- Changing adaptive tick rates or event-queue limits (unless measurement forces it).
- Full heap profiling tooling / leak detectors beyond bounding known unbounded fields.
- Widget API redesign or ratatui upgrade.
- Open-web ratatui/crossterm survey (Browser Use unavailable this session).
- Zero-copy `Line` sharing into `Paragraph` (needs a borrowed render path; residual extract clone is one per visible
  row).

## Tasks

- [x] T1: Add opt-in `frame_metrics` sampler + wire into `render_if_dirty` — acceptance: `VTCODE_TUI_FRAME_METRICS=1`
      logs windowed p50/p95 draw/input-to-draw every 5s; flag off adds no sampling work (covers: S2 frame metrics)
- [x] T2: Remove per-frame visible-window/header clones — acceptance: transcript render path no longer clones visible
      `Line`s for Paragraph when links empty; header built once per frame; existing transcript tests pass (covers: S2
      render fixes 1)
- [x] T3: Fix stream-append and eviction invalidation — acceptance: `append_text` does not clear `header_lines_cache`;
      eviction drops only evicted prefix of reflow cache; large-stream + eviction tests pass (covers: S2 render fixes 2)
- [x] T4: Fix hit-region per-cell alloc + input `mark_visual_dirty` alignment — acceptance: expand-hit region still
      found; hover/scroll do not rebuild header cache (covers: S2 render fixes 3–4)
- [x] T5: Bound PTY capture / activity entries / collapsed paste retention — acceptance: constants documented; growth
      tests show bounded memory under simulated flood (covers: S2 memory bounds)
- [x] T6: Verify + measure — acceptance: `cargo nextest run -p vtcode-ui` green in worktree; before/after draw_ms note
      in Report (covers: S2 testing)
