# Desktop⇄CLI Shortcut Parity Matrix

Source of truth for porting terminal keyboard shortcuts to VT Code Desktop.
CLI behavior: `docs/user-guide/keyboard-shortcuts.md` + `session/events.rs`
(composer) + `session/action.rs` (17 rebindable actions) + `app/session/`
overlay dispatch. Desktop: `crates/codegen/vtcode-desktop/src/desktop/view/`
(`shortcuts.rs` command table, `composer_*`, `palette*`, `queue.rs`,
`overlays.rs`). Kit baseline: vendored `gpui-base/input/base/state.rs:keymap`
plus `gpui-pre` dispatch (`keymap.rs`, `window.rs`).

## Dispatch rules (verified against vendored source)

- Bindings sort by **context depth desc, then registration index desc**;
  each matching binding's action is dispatched in order until
  `propagate_event` goes false (`resolve_bindings_for_input`,
  `dispatch_key_event`, `dispatch_action_on_node_inner`).
- Capture listeners **propagate by default** (must `stop_propagation` to
  claim); bubble (`.on_action`) listeners **stop by default** (must
  `propagate()` to fall through).
- Same-chord collisions therefore resolve to the highest-precedence action
  whose listeners don't all propagate: a deeper fixed binding **shadows** a
  user rebind rather than double-firing (hence `FIXED_RESERVED_CHORDS`).
- `Input` context is deeper than `VTCodeDesktop`, so Kit input bindings win
  first, then unhandled/propagated actions reach desktop bindings.
- `secondary-enter` = `cmd-enter` on macOS.

## Kit-native input keys (macOS, `Input` context)

Consumed by the focused input (never reach desktop): arrows/Home/End/paging,
Backspace/Delete + `cmd` line / `alt` word variants, `ctrl-a` line-start
(the later `MoveHome` registration shadows `SelectAll`), `ctrl-e` line-end,
`cmd+a` select-all, `cmd+x/c/v` + `ctrl` aliases, `cmd/ctrl+z` undo,
`cmd+shift+z`/`ctrl+y` redo, `cmd+up/down` buffer edges, `alt/ctrl+left/right`
word jump + `shift` select variants, `tab`/`shift+tab` indent/outdent,
`cmd+[/]` outdent/indent, `enter`/`shift-enter`/`secondary-enter`.

Propagated (reach desktop bindings): `escape` (nothing to dismiss),
`cmd/ctrl+.` (`ToggleCodeActions` unhandled), `cmd/ctrl+f`, `cmd+shift+f`,
and `ctrl+h` (`Search`/`Replace` propagate when not `searchable`).

Free chords (unbound in `Input`, available app-side): `ctrl+b/d/g/j/k/l/m/n/o/p/q/r/t/u/w`,
`alt+t/u/l/c/backquote`, `alt+\`, `alt+enter`, `alt+up/down`, `ctrl+enter` (`ctrl-s` now cycles history scope).

## Parity matrix

Legend: **native** = Kit handles with CLI-identical semantics; **done** =
desktop implements; **gap** = missing; **delta** = deliberate platform
difference; **oos** = out of scope (terminal-only).

### Composer submit / queue / newline

| CLI | Desktop | Status |
| --- | --- | --- |
| `Enter` submit, queue while running; scrolled + draft jumps to latest first | `handle_composer_enter` → `submit_or_jump_to_latest` + `enqueue_queued_submission` | done (`enter_with_draft_while_scrolled…` test) |
| `Ctrl+Enter` batchable queue | `cmd-enter` → `QueueBatched` | done (fixed secondary-Enter fallthrough; `cmd_enter_keypress…` test) |
| `Tab` accept suggestion first | `IndentInline` capture → suggestion accept, else focus-next | done |
| `Shift+Enter` newline | Kit `Enter{shift}` inserts newline | native |
| `Alt+Enter`, `Ctrl+J` newline | composer-context bindings → `Enter{shift}` | done (`composer_keys` test) |
| `\`+`Enter` submit | — | gap (terminal escape convention; low value on desktop) |
| `Alt+Up` pop newest queued into composer | `edit_queue` command (`alt-up`) → `take_latest_queued` | done (`alt_up_keypress…` test; CLI import maps `edit_queue`) |
| Ghost suggestion + `Alt+P` generate | inline suggestions via `MoveUp/Down` capture; no ghost/`Alt+P` | gap |

### History recall / picker

| CLI | Desktop | Status |
| --- | --- | --- |
| `Up/Down` single-line composer → history prev/next (live draft stashed) | `recall_history` via `MoveUp/Down` capture (single-visual-row gate, clamps, stash restore) | done (`history_recall_traverses…` test) |
| `Ctrl+P/N` always traverse | `HistoryPrevious/Next` composer-context bindings | done (`ctrl_p_recalls_from_multiline…` test) |
| `Ctrl+R` history picker | `ctrl-r` → `OpenPromptHistory` (draft-prefilter, `Enter` accept, `Esc` close, first-match preselect); rows matched/ranked by the shared `vtcode_commons::prompt_history` nucleo ranker, so multi-token queries like `impl chg` span the phrase | done (`ctrl_r_opens_prefiltered…`, `history_search_matches_non_contiguous…` tests) |
| History scope (Everywhere/Project/Session) + `Ctrl+S` cycle | fixed `ctrl-s` → `CycleHistoryScope` (opens when closed) + segmented switcher in the palette header; scope kept across opens | done (`ctrl_s_cycles_scope…`, `scope_switcher_click…` tests) |
| Picker cycle (`Tab`/`Shift+Tab`), `Ctrl+R`, `Ctrl+P/N` nav | Kit `Command` `Up/Down/Enter/Esc` + `HistoryPrevious/Next` capture; `Tab`/`Shift+Tab` and a repeat `Ctrl+R` cycle matches; `Ctrl+G` cancels and restores the draft | done (`ctrl_r_cycles…`, `tab_cycles_history…`, `ctrl_g_cancels…` tests) |
| Picker cross-thread prompts (`Everyone/Project/Session` scopes, 200 cap) | `session/recent-prompts` merged under the live thread ("Other threads" section); each section ranked by the shared ranker and deduped against the live thread; one merged 200-row budget split live-thread-first so recents cannot be starved or double the budget; silent on older services | done (`prompt_history` tests) |
| Picker attachment restore | text-only | gap (needs protocol surface) |

### Line editing / readline

| CLI | Desktop | Status |
| --- | --- | --- |
| `Ctrl+A/E` line edges; `Cmd+Left/Right`; `Home/End`; `Cmd+Up/Down` buffer edges | Kit bindings | native |
| `Alt+Left/Right` word jump; `Alt+Backspace/Delete` word delete | Kit bindings | native |
| `Ctrl+Z/Y` undo/redo | Kit bindings | native |
| `Ctrl+F/B` char move | composer-context bindings → `MoveRight/Left` | done (`composer_keys` test) |
| `Ctrl+W`, `Alt+D`, `Ctrl+U`, `Ctrl+K` | composer-context bindings → Kit delete actions (undo-preserving) | done (`composer_keys` tests incl. undo round-trip) |
| `Ctrl+T`, `Alt+T/U/L/C`, `Alt+\` transpose/case/whitespace | — | gap (no Kit primitive; hand-rolled surgery would break undo) |
| `Ctrl+G` external editor | — | gap (needs desktop editor story) |
| `Ctrl+M` model picker | composer-context `ctrl-m` → `OpenModelPicker` | done (`ctrl_m_opens…` test) |
| `Cmd+A`/`Cmd+Backspace` clear input; `Cmd+E` line end | `cmd+a` = Select All; `cmd+backspace` = delete-to-line-start | delta (macOS idiom; clear moves to double-Esc) |
| `Esc Esc` clear draft / rewind picker | double-Esc clears draft text (snapshot disarm); no rewind surface | done (`double_escape…` tests) |

### Agent / mode / quick commands

| CLI | Desktop | Status |
| --- | --- | --- |
| `Shift+Tab` cycle agent | `OutdentInline` capture → suggestion-up when open, else `cycle_primary_agent` (mode-switch lock + busy notice); multiline drafts keep Kit's native outdent (CLI always cycles) | done (`shift_tab_cycles…`, `shift_tab_on_multiline…` tests) |
| `?` shortcut overlay | Settings→Shortcuts page | delta |
| `@` file/subagent, `/` slash triggers | triggers + slash routing + suggestion keyboard | done |
| `!` bash, `#` prompts triggers | unverified | gap |
| File/agent palette: `Up/Down`, `Enter`, `Esc` | Kit `Command` + suggestion capture | done |
| Palette `Tab` select-best, `BackTab` up | `Tab` accept; `BackTab` → suggestion-up via capture | done |
| Palette `PgUp/PgDn`, file `←` ascend, `Alt+Enter` folder-ref, digits `1–9` | — (HITL panel has digits) | gap |

### Cancel / Escape

| CLI | Desktop | Status |
| --- | --- | --- |
| `Ctrl+C` interrupt, no auto-resume | `cmd+.` → `CancelRun` (falls through from `Input`) | done (`keyboard_shortcuts_share_session_and_cancellation_guards`) |
| `Esc` dismiss topmost, restore focus | `dismiss_overlay` chain + `*_return_focus` | done (overlay/palette/preview tests) |
| `Esc` from transcript → composer | `transcript_focus` branch in `dismiss_overlay` | done (`transcript_arrows…` test) |
| `Ctrl+D` exit; double `Ctrl+C` exit | window/menu semantics own this | oos |

### Transcript / review / scroll

| CLI | Desktop | Status |
| --- | --- | --- |
| Message-list `Up/Down` row nav | `VTCodeTranscript` context: highlight + reveal + `scroll_to_item`, clamps, skips hidden rows | done (`transcript_arrows…`, `transcript_nav_skips…` tests) |
| Review route vim keys (`/`, `n/N`, `j/k`, `g/G`, `r`, `v`, `q`) | review route + `F7` nav only | gap |
| `PgUp/PgDn` scroll; `Ctrl+End` jump-to-last-change | scroll regions exist; no jump target | gap |
| `Ctrl+L` clear screen | no transcript-clear surface | gap (needs product decision) |
| `F8` diagnostics, `Alt+G` task panel, `Alt+S` subprocesses | inspector/terminal/output tabs exist | gap (needs command bindings) |

### Threads / panels / experimental vim

| CLI | Desktop | Status |
| --- | --- | --- |
| Thread-list keyboard nav | `VTCodeSessions` context: `Up/Down/Home/End` highlight + focus rail, `Enter` loads via the row-click path (running-confirm guards shared) | done (`session_arrows…`, `session_enter…`, `session_click…` tests) |
| Vim list motions | `j/k/g/G` in threads + transcript, gated on the experimental Settings toggle (off by default, persisted) | done (`vim_motions…`, `vim_keys_are_inert…` tests) |
| Vim tab motions | `h/l` cycle the inspector + settings rails under the same gate | done (`vim_hl…` test) |
| Vim `i` / `/` | focus composer / open conversation search under the same gate; typing in inputs never triggers vim actions | done (`vim_i…`, `vim_keys_typed…` tests) |
| Reset keyboard config | Reset restores shortcut defaults (staged, Save persists) and turns the vim toggle off (immediate, no draft) | done (`vim_toggle_persists…` test) |

## Shipped in this increment

1. Fixed: `cmd-enter` while running queued standalone (`batchable=false`).
2. New commands: `prompt_history` (`ctrl-r`), `edit_queue` (`alt-up`) — 33 total, localized ×20.
3. New key contexts: `VTCodeComposer` (editing + recall + `ctrl-m`), `VTCodeTranscript` (row nav).
4. Fixed: palette first-select clobbered filtered selections (`focus_palette_state` guard).
5. New behaviors: inline history recall, Shift+Tab agent cycle, double-Esc draft clear, transcript row nav.

## Remaining gaps (ranked)

1. Medium: palette `PgUp/PgDn`/folder-ref/digits; review-route vim keymap (the desktop vim layer covers
   threads/transcript/tabs only — review stays F7-only).
2. Medium: transpose/case edits (needs Kit primitives or undo-safe surgery).
3. Low: `!`/`#` triggers verification; `Ctrl+L`/`F8` product decisions; jump-to-last-change.
4. Service gaps (protocol work): archived-prompt history, rewind, external-editor round-trip.
