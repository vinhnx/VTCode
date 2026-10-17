---
feature: config-models-modal-ux
status: delivered
updated: 2026-09-26
branch: compose/config-models-modal-ux
commits: 4c2061337..772b9441e
---

# Config & Models Modal UX

## Report

**What was built** — `/config` and `/model` now share one presentation contract: semantic `InlineTone` badges, accent
live values, dimmed descriptions, and an in-modal status strip plus a short chat line for every apply/save.

Config palette rows render **title + accent value + dimmed description**. Booleans show state badges (`On`/`Off` with
Success/Neutral tone); actions use Accent/Danger as appropriate. Nested views open with `← Back`; `Esc` backs out. Reset
confirmation defaults to **Keep settings** and marks **Reset everything** as Destructive. Breadcrumbs use `›`. The
keyboard footer matches the real keys.

The model picker shows `Step 1 · Model` / `Step 2 · Reasoning` headers, provider section headers, a `Current` badge
(tone `Current`) on the active model, capability metadata as dimmed subtitle, and `← Back to model list` on follow-up
steps. Completion prints one chat line: `Model: provider/model · reasoning · tier`. Persist failures emit
`Could not save model selection: …` before propagating.

Shared plumbing: `InlineTone` / `InlineItemKind` / `InlineStatus` in `vtcode-commons`, `value`/`badge_tone`/`kind` on
`InlineListItem` (Default + builders), tone mapping and value rows in `modal_list_item_lines`, status strip above the
keyboard hint with hit-test geometry (`summary_line_rows(.., has_status)`).

**Verification** — commands run and observed results:

- `./scripts/check-dev.sh` — PASS (fmt, clippy `-D warnings`, compile, shell lint)
- `cargo nextest run -p vtcode-ui` — PASS (1387)
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E 'test(settings) or test(model_picker) or test(palette) or test(modal) or test(mimo_auth) or test(service_tier)'`
  — PASS (147)
- New tests: `setting_row_renders_accent_value_and_dimmed_subtitle`, `status_strip_uses_tone_and_sits_above_hint`,
  `badge_tone_maps_current_to_accent_bold`, `summary_line_rows_count_status_strip` — PASS
- PRE-EXISTING: `prompts::system::tests::test_golden_multi_section_output_is_byte_identical` (env-dependent skills list
  in prompt golden; `system.rs` untouched)

Independent review found missing-doc `<unset>` drop, absent save-failure feedback, missing per-step back, and dual
completion lines; all fixed and re-reviewed (no remaining criticals).

**Journey log** —

1. Mechanical `InlineListItem` field insertion via regex hit return-type `-> InlineListItem {` and `impl InlineListItem`
   — several files needed restore + a tighter script. Lesson: skip `->` and `impl` prefixes when patching struct
   literals.
2. Accidental `git stash -u` mid-review emptied the worktree; recovered with `git stash pop`. Keep feature work
   committed early on a branch.
3. `setting_subtitle(_summary, …)` dropped the summary argument as a side effect of the value/description split —
   missing-doc rows lost `<unset>` until the value slot was wired.
4. ConfigAction lists force `CONFIG_LIST_NAVIGATION_HINT` and ignore an explicit footer; changing the shared constant
   was the only way to update the keyboard copy.
5. `Current` belongs in a badge/tone, not packed into subtitle text — keeps metadata dimmed and the selection
   glanceable.

## [S1] Problem

The `/config` settings palette and `/model` picker share `InlineListItem` and a common modal list, but the rendered rows
have no cohesive visual hierarchy. Titles, values, descriptions, and badges compete at similar weight; subtitles pack
`value • description` into one dimmed string so the live value is hard to spot; badges are free-form text with ad-hoc
bold/italic exceptions; headers and breadcrumbs are plain strings. After a change, feedback is a `MessageStyle` line in
the chat transcript (`Enabled X`, `Saved to vtcode.toml`, warnings) which scrolls away while the modal stays open, so
users cannot tell whether their last keystroke applied or failed.

Interaction flows have the same gaps: the config reset path is a bare confirm view without danger emphasis; free-form
string editing is an implicit inline editor with weak commit/cancel cues; the model picker's multi-step flow (model →
reasoning → service tier → API key) does not show step progress; provider/model rows do not group visually; keyboard
hints are inconsistent between `CONFIG_LIST_NAVIGATION_HINT` and `MODEL_PICKER_*` copy.

User-confirmed direction: **full interaction redesign** of both modals, with **in-modal status + a short chat line** for
feedback.

## [S2] Design

### Decisions

1. **Shared presentation model** — extend `InlineListItem` with optional presentation fields (value, tone, kind) plus
   `Default`; one style-mapping path in `modal_list_item_lines` so TUI and inline surfaces cannot diverge.
2. **Semantic tones** — badges and status use a closed tone enum (Neutral, Accent, Success, Warning, Danger, Current)
   mapped onto theme styles; free-form badge strings stay as the label, tone is explicit data.
3. **Feedback** — every apply/save/cancel/error sets a modal status strip (tone + message) that persists inside the
   modal until the next action or view change, and emits one short chat transcript line for durable record.
4. **Config flows** — clearer nested navigation (breadcrumb + explicit Back), value-first rows, typed badges that
   reflect state, danger-styled reset confirmation, visible inline editor commit/cancel, consistent keyboard footer.
5. **Model flows** — step progress in the header, provider section headers, current-model emphasis, dimmed capability
   metadata, follow-up steps that name the chosen model, completion status that names the final model + reasoning +
   tier.

### Shared presentation contract

**`vtcode_commons::ui_protocol`** (`selection.rs` + new tone/kind types):

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineTone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    Current,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineItemKind {
    #[default]
    Item,      // selectable row
    Header,    // non-selectable section header (already detected via selection=None + is_header)
    Setting,   // config key row (title = label, value = live value)
    Action,    // imperative row (Reset, Reload, Back, Pick model…)
    Hint,      // non-selectable note
}

pub struct InlineListItem {
    pub title: String,
    pub subtitle: Option<String>,   // description / metadata only (dimmed)
    pub badge: Option<String>,      // short label; tone from `badge_tone`
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
    // NEW — optional presentation
    pub value: Option<String>,      // live value, accent-styled; replaces value-in-subtitle packing
    pub badge_tone: InlineTone,
    pub kind: InlineItemKind,
}
```

Construction rules:

- Derive `Default` on `InlineListItem`; new fields default to `None` / `Neutral` / `Item`.
- Add `InlineListItem::new(title, selection)` helper and `with_value` / `with_badge(label, tone)` / `with_subtitle` /
  `with_kind` builders.
- Mechanically add `..Default::default()` to existing full struct literals (compile-driven; no behavior change for
  untouched surfaces).
- Existing callers that only set `title/subtitle/badge/indent/selection/search_value` keep rendering as today until they
  opt into `value` / `badge_tone` / `kind`.

**Render mapping** (`modal_list_item_lines` in `crates/codegen/vtcode-ui/src/tui/core_tui/session/modal/render.rs`):

| Row part                   | Style rule                                                                                                                                              |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Cursor / selection gutter  | existing highlight                                                                                                                                      |
| Shortcut number `1.`–`9.`  | `detail` (dimmed)                                                                                                                                       |
| `badge` label              | `badge_tone` → theme badge style; `Current`/`Success` bold, `Danger` bold+danger fg, `Warning` warning fg, `Accent` accent fg, `Neutral` `styles.badge` |
| `title`                    | selected+selectable → `highlight` (bold accent); selectable → `selectable`; `Header`/`Hint` → `header`/`detail`                                         |
| `value`                    | accent/highlight fg, bold when selected; rendered after title as `Title  Value` or trailing-aligned when width allows                                   |
| `subtitle`                 | `detail` (dimmed); no longer contains the live value                                                                                                    |
| Divider / blank header gap | existing                                                                                                                                                |

Badge tone lookup is **data-driven** (`badge_tone`), not string matching. Keep `modal_badge_style` string special-cases
only as a fallback for callers that did not set a tone.

### Status strip contract

**Protocol** (`vtcode_commons::ui_protocol`):

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineStatus {
    pub tone: InlineTone,          // Success | Warning | Danger | Info(as Neutral/Accent)
    pub message: String,
}
```

- `ListOverlayRequest` / `show_list_modal_with_footer` gain `status: Option<InlineStatus>`.
- `ModalState` stores the latest status; `modal_list_summary_line` / footer region renders it as a dedicated row
  **above** the keyboard hint (or replacing the summary line when present).
- Status row format: `• {message}` with tone color; truncation uses `truncate_modal_text` and must show `…`.
- Config/model handlers set status after every apply; clearing rules: new apply overwrites; `Esc` at root / close does
  not need to clear (modal is gone); navigating into a group keeps the status so the user still sees "Enabled X".

**Chat line** (durable record):

- Success: `MessageStyle::Info` — one line, e.g. `Settings: enabled IDE context` /
  `Model: anthropic/claude-sonnet-5-5 · reasoning high`.
- Warning: `MessageStyle::Warning` — including "saved but runtime config not reloaded".
- Error: `MessageStyle::Error` — apply/save failures with a short reason and the remediation hint when known.
- Cancel/close: no chat line (stay quiet).

### Config palette interaction redesign

Surfaces: `src/agent/runloop/unified/settings_interactive/*` (item builders, headers, actions) + shared render above.

1. **Header block** (instruction lines):
   - Line 1 (header style): breadcrumb `Settings › {Group} › {Nested}` with `›` separators; root is `Settings`.
   - Line 2 (detail): one-sentence purpose of the current view (existing summaries).
   - Line 3 (detail, dimmed): `Write target: {short_path}` + optional source label. Never competing with the breadcrumb.
   - Reset confirmation view: title `Settings › Reset`, body explains blast radius, write target, and "Credentials are
     preserved."

2. **Row model** (curated groups, advanced, nested):
   - `kind: Setting` + `value: Some(summarize_value)` + `subtitle: description only`.
   - `kind: Action` for Reload / Reset / Back / Pick model / Setup editor.
   - `kind: Header` for section labels and group titles at root (group titles remain selectable `Action`s that open; use
     `kind: Action` with `badge: Group`).
   - Badges become semantic: `On`/`Off` → Success/Neutral with label reflecting **state** (`badge: "On", tone: Success`
     vs `badge: "Off", tone: Neutral`); `Step`/`Edit`/`Pick`/`List` → Accent; `Reset` action → Danger tone on the badge.

3. **Navigation**:
   - First row in any nested view is always `← Back` (`kind: Action`, `badge_tone: Neutral`), subtitle names the parent
     (`Settings` or parent group).
   - `Esc` = Back in nested views; `Esc` at root closes the palette (existing).
   - Selection memory per view (`selection_by_view`) is preserved.

4. **Value interactions** (keyboard footer text must match behavior):
   - Toggle: `Enter`/`Space` toggles; status `Enabled {title}` / `Disabled {title}`.
   - Cycle: `Enter`/`Space`/`→` next, `←` previous; status `{title} → {value}`.
   - Numeric: `←`/`→` step; status `{title} → {value}`.
   - Free-form string: `Enter` opens inline editor row (label + placeholder + caret); `Enter` commits, `Esc` cancels
     editor without closing modal; status on commit.
   - Table/Array: `Enter` opens nested view; Back returns.
   - Reset action: `Enter` opens confirm view with exactly two rows — `Reset everything` (`kind: Action`,
     `badge_tone: Danger`, badge `Destructive`) and `Keep settings` (`kind: Action`). Default selection is **Keep
     settings**. Confirm runs reset and sets Danger→Success status `Reset configuration at {path}`.

5. **Footer / keyboard hint** (single source of truth, shared constant):
   - `↑↓ navigate • Enter/Space apply • ←→ change value • type to filter • Esc back/close`
   - When the inline editor is active: `Enter save • Esc cancel edit`.
   - Replace `CONFIG_LIST_NAVIGATION_HINT` copy to match; keep ConfigAction lists `FixedComfortable` density.

### Model picker interaction redesign

Surfaces: `src/agent/runloop/model_picker/*` (rendering, prompts, state_machine) + shared render.

1. **Header / step progress**:
   - Title stays `Model` / `Reasoning` / `Service Tier` / `API key` as today (wizard tabs already exist for TUI wizard
     flows; for the list-modal steps use header lines).
   - Header line 1: `Step {n} · {Step title}` (dimmed step index, bold step title) when the flow has follow-up steps
     pending or in progress.
   - Header line 2: `Current: {provider} / {model}` with `Current` as an accent `InlineTone::Current` marker in the
     header text; follow-up steps add `Selected: {model_display}`.

2. **Step 1 (model list)**:
   - Provider **section headers** (`kind: Header`, non-selectable): `Anthropic`, `OpenAI`, `Merge Gateway`, … using
     existing `provider.label()`. Skip headers for providers with no rows.
   - Model rows: `title` = display name, `value` unused, `subtitle` = capability metadata only
     (`200K context • Reasoning • Tools • image`) via existing `static_model_capability_segments`; move `Current` out of
     the subtitle string into `badge: Current, badge_tone: Current` (replacing the provider badge on that row) or a
     trailing `value` of `Current`.
   - Provider badge on non-current rows: `badge: provider.label(), badge_tone: Neutral` (or Accent for custom/dynamic).
   - Dynamic-unavailable rows (`server not running`) keep their setup action; `badge_tone: Warning`.
   - Codex runtime note stays a header/hint line (`kind: Hint`), dimmed, not a selectable row.
   - Footer: existing `MODEL_PICKER_NAVIGATE_FILTER` (already correct) — ensure ConfigAction density does not swallow it
     (pass as `footer_hint` with `Adjustable` density for this list, or lift the filter so model lists keep the hint).

3. **Step 2 (reasoning)**:
   - Header: `Selected: {model_display}` + `Current reasoning: {label}`.
   - `Keep current ({label})` is first row, `badge: Current, badge_tone: Current`, preselected.
   - Level rows: `kind: Item`, subtitle = description; GPT-5 `None` badge `Accent`.
   - Reasoning-off alternative row: `badge: No reasoning, badge_tone: Neutral`.
   - Footer: `MODEL_PICKER_FOLLOW_UP_HINT` plus `Backspace/Esc` is cancel-picker today — document in footer:
     `Esc cancels the picker` (no silent per-step back unless T4 adds it). **Add per-step back**: `←` or `Backspace`
     returns to model list without cancelling the picker; footer lists `← back` when not on step 1.

4. **Step 3 (service tier)** and **API key**:
   - Same header pattern (`Selected: …`).
   - API key secure prompt label: `API key for {provider}` + dimmed `Stored via {env_key or credential store}`.
   - Footer hints name the action verbs (`Enter save`, `Esc skip` when skip is legal).

5. **Completion feedback**:
   - Status (if modal still open for a follow-up) and always a chat line:
     `Model: {provider}/{model_id} · reasoning {label}` (+ `· tier {tier}` when set).
   - Persist failure: chat `Error` line + status `Could not save model selection: {err}`; picker stays open on the
     failing step.

### Style tokens

Use existing theme / `ModalRenderStyles` fields (`highlight`, `badge`, `header`, `selectable`, `detail`, `hint`,
`search_match`). Do **not** introduce a parallel palette. Tone mapping:

| Tone    | Style source                                                                                                                                                   |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Neutral | `styles.badge` / `styles.detail`                                                                                                                               |
| Accent  | `styles.highlight` (no selection bg)                                                                                                                           |
| Success | `styles.highlight` bold (or theme success if available in `ModalRenderStyles`; add `success`/`warning`/`danger` fields only if the theme already exposes them) |
| Warning | `styles.hint` or theme warning                                                                                                                                 |
| Danger  | theme danger / red fg, bold                                                                                                                                    |
| Current | `styles.highlight` bold                                                                                                                                        |

If `ModalRenderStyles` lacks semantic success/warning/danger slots, add them and populate from the active theme in the
same change that introduces tone mapping; all built-in themes must keep WCAG AA contrast (existing `vtcode-ui` theme
tests).

### Error / success prominence

- Status strip is always the **last content row before** the keyboard hint (or the summary line slot), never buried in
  the instruction block.
- Errors use `Danger` tone + bold; successes use `Success` tone; warnings use `Warning`.
- Chat lines follow `MessageStyle::{Info,Warning,Error}` as today but with the shorter, labelled wording above
  (`Settings: …` / `Model: …`).
- Apply failures never leave the modal in a half-edited state: draft mutations remain transactional
  (`mutate_draft_and_persist` already is); on error, reload draft from disk or keep last valid draft and set status to
  the error.

## [S3] Out of Scope

- Other list modals (permissions, rewind, sessions, slash palette, plan approval) — they inherit improved shared styling
  only where fields are set; no interaction redesign there.
- Theme engine / palette token overhaul beyond wiring tone → existing theme styles.
- Plain (non-inline) CLI fallbacks beyond keeping them compiling and behaviorally unchanged.
- Changing `ThreadEvent` / tool schemas / config file schema.
- i18n, mouse hit-testing changes beyond keeping `hit_test` row math consistent with the new status/footer rows.
- Subagent model shortcuts UI (`model_picker/subagent.rs`) except where shared item construction requires
  `..Default::default()`.

## Tasks

- [x] T1: Shared presentation model — add `InlineTone`, `InlineItemKind`, `InlineStatus`, extend `InlineListItem` with
      `value`/`badge_tone`/`kind` + `Default` + builders; mechanically update existing literals to
      `..Default::default()`; map tones and `value`/`subtitle` split in `modal_list_item_lines`; keep `hit_test` row
      geometry in sync. — acceptance: workspace compiles; a Setting row with `value` renders title + accent value +
      dimmed subtitle; badge tone colors match the table; existing unmodified items look unchanged (covers: S2)
- [x] T2: Status strip + chat-line helper — `InlineStatus` on `ListOverlayRequest`/`show_list_modal*`; store on
      `ModalState`; render status row above keyboard hint with tone styling and truncation; add
      `status_message_line(tone, message)` helper used by config/model handlers. — acceptance: opening a modal with
      `status: Some(…)` shows the toned row; next action replaces it; summary/hint rows still reserve the correct
      heights in `summary_line_rows` (covers: S2; depends: T1)
- [x] T3: Config palette interaction redesign — breadcrumb/headers/write-target hierarchy; Setting rows use `value` +
      semantic badges (On/Off state, Edit/Step/Pick, Reset=Danger); always-visible Back row; align keyboard footer with
      actual keys (toggle/cycle/step/editor/reset); inline editor commit/cancel cues; reset confirm with Keep-settings
      default + Destructive badge; wire every `SettingsApplyOutcome` to status + short chat line. — acceptance:
      toggle/cycle/step/editor/reset each set a status and a chat line; Esc in nested view goes Back; reset default
      selection is Keep settings (covers: S2; depends: T1, T2)
- [x] T4: Model picker interaction redesign — step progress headers; provider section headers; current-model
      badge/tone + capability subtitles without value-in-subtitle packing; reasoning/service-tier/API-key follow-ups
      show `Selected: …`; per-step `←` back without cancelling picker; footer hints list `← back` when applicable;
      completion + failure status/chat lines name model, reasoning, and tier. — acceptance: step 1 groups by provider
      and marks Current; step 2 preselects Keep current and can return to step 1; completing prints `Model: …` chat
      line; save failure keeps picker open with Danger status (covers: S2; depends: T1, T2)
- [x] T5: Tests + docs — unit/PTY tests for tone mapping, status row geometry, config action outcomes (status + chat
      line strings), model step back, and current-badge rows; update `docs/development/` quick-reference for `/config`
      and `/model` interaction/feedback; run theme contrast tests if `ModalRenderStyles` gained slots. — acceptance: new
      tests fail on base and pass after; docs mention status strip and keyboard model; `./scripts/check-dev.sh` green
      (covers: S2)
