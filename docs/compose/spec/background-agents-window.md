---
feature: background-agents-window
status: delivered + follow-up
updated: 2026-10-10
branch: feat/background-agents-window
commits: 4f90643b0..e988dd537
---

# Background Agents Expanded Window

## Follow-up 2026-10-10 — inline bottom-dock replaces the centered window

The centered floating window covered the transcript, so `LocalAgents` moved back to `BottomDocked`: a compact
inline panel above the composer (transcript stays visible), with `Ctrl+E` or a header click expanding it to ~75% of
the available panel height (`Ctrl+E` again collapses). List+detail 38/62 split, header counts, and all management keys
are unchanged; the info line now advertises `Ctrl+E expand/collapse`. The transcript progress row is no longer occluded
while the panel is open. Commits `efe7a47ea` (dock + expand) and `6737eec48` (header-click mouse coverage).
Verification: `cargo nextest run --locked -p vtcode-ui` — PASS (1571); `./scripts/check-dev.sh` — PASS.

## Report

**What was built** — The Local Agents drawer is now a centered floating **Background** window for multi-agent /
background-process management. The left pane lists delegated agents, background subprocesses, and retained exec sessions
(live rows first; finished rows stay visible with terminal status). The right pane shows the selected entry's status,
summary, transcript path, and live preview tail. The window header reports `N running · M finished`, or
`N agents finished` when nothing is live.

Finished history is retained: delegated `Completed`/`Failed` stay listed (`Closed` stays hidden as a user dismissal),
and background `Stopped`/`Error` rows remain. The input status indicator shows `Running N background task(s)...` while
work is live and `N agent(s) finished` afterwards. Clicking that activity text or the `{key} background` hint toggles
the window; `Ctrl+B` / `Alt+S` / `/subprocesses` / Down still open it and Esc closes it. Existing
inspect/stop/close/focus/transcript actions are unchanged.

**Verification** — commands run and observed results:

- `cargo nextest run -p vtcode-ui` — PASS (1268)
- `cargo nextest run -p vtcode -E 'test(/session_setup/) or test(/local_agent/) or test(/status_refresh/)'` — PASS (69)
- `./scripts/check-dev.sh` — PASS (fmt, clippy `-D warnings`, compile, shell lint)
- Independent reviews: first pass flagged over-broad click hit regions (whole combined hint + merged ranges) and weak
  negative/count tests; re-review confirmed those fixed but found the key-span recorder overwrote its own range so only
  `background` was clickable; second fix extends `{key} background` as one range and asserts both ends are hot while
  `Alt+S local agents` misses.

**Journey log** —

1. User chose expanded management window (not full Claude Code metric cards / message rows); list+detail layout; keep
   finished visible; click indicator + shortcuts; replace the drawer.
2. `TransientSurface::LocalAgents` moved to `FloatingModal`; `BottomPanelKind::LocalAgents` and the bottom-docked
   splitter were removed. Paint goes through `render_local_agents(viewport)` and records `window_area`/`list_area` for
   mouse hits.
3. Status indicator hits are a `Vec<Rect>` (disjoint), not one min/max-merged rect. Only the activity text and
   `{key} background` are hot; `Alt+S local agents` is not.
4. `LocalAgentEntry::is_finished` is `!is_loading()` for header counts, not a positive terminal-status match.
5. Spec S2 originally listed `Closed` as retained; T1 and the implementation hide it. Text aligned to T1.
6. Post-delivery review: auto-opened window must close when live delegated work ends (not when history empties); hint
   hit widths use `measure_text_width`; `Rect::contains` replaces manual hit math; hits past truncation are dropped.

## [S1] Problem

VT Code can run multiple delegated subagents, background subprocesses, and retained exec sessions at once, but the TUI
surface for that work is a compact bottom-docked Local Agents drawer (list + tiny preview). When several agents are live
the drawer is too small for real management: users cannot scan status at a glance, read live activity without opening
each item, or tell which agents finished. The status line only shows `Running N background tasks...` with a keyboard
hint; there is no click target and no expanded view.

Reference UX (Claude Code screenshots): agent cards with live status, a large expanded background window showing running
command/tool activity, a finished-state summary (`N agents finished`), and per-agent detail. This feature owns the
**expanded management window** surface only — not metric cards, not collapsible transcript message rows.

## [S2] Design

Decision (user-confirmed):

- **Primary surface** = expanded management window replacing the Local Agents drawer (one surface, not two).
- **Layout** = list + detail pane (left: agents/processes; right: selected entry status, summary, transcript path, live
  preview).
- **Finished agents stay visible** with terminal status (`completed` / `failed` / `stopped` / `error`) and a
  finished-count summary.
- **Open** via click on the background activity indicator in the input status line, plus existing shortcuts (Ctrl+B,
  Alt+S, `/subprocesses`, Down when entries exist). Esc closes.

### Surface contract

`TransientSurface::LocalAgents` moves from `BottomDocked` to `FloatingModal` (same placement family as DiffPreview /
ToolOutputViewer). It remains `CapturedInput` focus. Opening uses the existing `open_local_agents_drawer` /
`TransientRequest::LocalAgents` path; only layout and entry retention change.

### Layout

```text
┌─ Background ──────────────────────────────────────────┐
│ 2 running · 1 finished                                │
│ ↑↓ Navigate · Enter inspect · Alt+O transcript · ...  │
├────────────────────┬──────────────────────────────────┤
│ › Fix llm review   │ Fix vtcode-llm review findings   │
│   delegated · run  │ delegated · running              │
│   Fix core review  │ Summary: Reviewing the workspace │
│   delegated · done │ Transcript: /path/to/transcript  │
│   Bash: cargo test │                                  │
│   exec · run       │ $ cd ... && cargo test ...       │
│                    │ test result: ok. 285 passed      │
└────────────────────┴──────────────────────────────────┘
```

- Left list (~38%): `display_label · kind · status · id` rows, selection cursor, agent color on divider.
- Right detail (~62%): title line, status (shimmer while loading), summary, transcript path, blank line, live preview
  tail (existing `preview` string).
- Header: counts — `N running · M finished` when any terminal entries remain; `N agents finished` when nothing is live
  but history is visible.
- Empty state: keep existing opt-in copy (configure background agent / Ctrl+B / `/subprocesses`).

### Entry retention (queue / process visibility)

Change the `visible_*_local_agents` filters in `src/agent/runloop/unified/session_setup/ui/local_agents.rs`:

- **Delegated**: keep `Queued | Running | Waiting | Completed | Failed`. `Closed` stays hidden (user dismissal). Sort
  still `updated_at` descending so live work floats up.
- **Background subprocess**: keep `Starting | Running | Stopped | Error` (stop dropping terminal).
- **Exec sessions**: unchanged (already retained while managed).

`LocalAgentEntry::is_loading` stays as the live-work predicate (`queued|running|waiting`, `starting|running`,
`running`). `background_activity_count` continues to use `loading_count()` only.

Optional simple metrics stay out of the data model. `summary` already carries human context.

### Open / close triggers

1. **Click** on the background activity status text (`Running N background task(s)...`) or the `{key} background` hint
   span in the input status line → open window. Hit-test the spans recorded for those labels; do not open on unrelated
   status text.
2. **Keyboard** (unchanged): Ctrl+B, Alt+S, `/subprocesses`, Down when entries exist and composer is empty.
3. **Auto-open** on first new delegated entry (unchanged policy).
4. **Esc** / existing close keys close the surface. Click outside is not required.

### Management actions (unchanged keys, now on the expanded surface)

- ↑↓ / PgUp / PgDn / Ctrl+N — navigate list
- Enter — inspect (exec sessions / open detail action)
- Alt+O — open transcript when `transcript_path` is set
- Ctrl+K — stop / graceful terminate
- Ctrl+X — force close
- Ctrl+R / Ctrl+P — focus / preview (exec sessions only)
- Esc — close window

### UI module changes

- `crates/codegen/vtcode-ui/src/tui/core_tui/app/session/transient.rs`: `LocalAgents` → `FloatingModal` placement.
- `.../app/session/render/local_agents.rs`: render as centered floating panel (Clear + bordered block), split ~38/62
  list/detail, header counts. Reuse `SharedListWidgetModel` for the list; detail body stays a `Paragraph`.
- `.../app/session/layout.rs` / `impl_render.rs`: route LocalAgents through floating-modal layout instead of
  `split_inline_local_agents_area` bottom dock.
- `.../session/input.rs`: record clickable ranges for background status / hint spans; mouse click opens the window.
- Keep `format_local_agent_preview` / title / status helpers; add header count line.

### Error behavior

- Failed agents keep `error` text in summary/preview (existing fallback).
- Missing transcript path hides the transcript line.
- Zero-width / zero-height terminal: skip render without panicking (existing guards).

## [S3] Out of Scope

- Tool-use / token metric cards in the status bar or agent rows.
- Collapsible `Message from @id` rows in the main transcript.
- Separate Messages / Processes tabs.
- Cross-agent queue reordering or priority controls.
- Changing subagent concurrency, spawn, or runloop scheduling.

## Tasks

- [x] T1: Retain terminal delegated/background entries and add finished/live counts — acceptance:
      `build_local_agent_entries` keeps completed/failed/stopped entries; `LocalAgentsState` exposes live and finished
      counts (covers: S2)
- [x] T2: Floating list+detail layout for LocalAgents — acceptance: surface renders as centered modal with list/detail
      split and `N running · M finished` / `N agents finished` header; Esc closes (covers: S2; depends: T1)
- [x] T3: Click-to-open on background status indicator — acceptance: mouse click on `Running N background task(s)...` or
      `{key} background` hint opens the window; other status text does not (covers: S2; depends: T2)
- [x] T4: Keyboard/action parity + tests — acceptance: existing LocalAgents keys and actions still work on the floating
      surface; unit tests cover retention, counts, layout header, and click hit-test (covers: S2; depends: T2, T3)
