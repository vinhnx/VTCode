---
feature: ui-design-system
status: delivered
updated: 2026-09-29
branch: compose/ui-design-system
commits: b2efeb1b6..ac8905a82
---

# Canonical TUI Design System in vtcode-ui

## Report

**What was built** — `vtcode_ui::design` is the canonical TUI design system for list UIs. `design::list` provides the
row factory (`group_header`, `group_divider`, `hint`, `setting`, `action`, `choice`, `current_choice`); `design::keys`
owns keyboard/interaction copy (`list_hint()`, `choice_hint()`); `design::constants::VALUE_COL` drives the value column.
Wire types stay in `vtcode_commons::ui_protocol`.

Settings, model picker, slash-command menus (plugins, skills, agents, oauth, mcp, secrets, …), palettes,
permission/limit prompts, and most plan/inline/request_user_input surfaces build rows through those factories (via
`src/agent/runloop/ui_list`). Group rhythm (blank above and below headers) and value-column alignment live in one
renderer. Footer hints and standalone-picker keys come from `design::keys`.

**Verification** — commands run and observed results:

- `./scripts/check-dev.sh` — PASS
- `cargo nextest run -p vtcode-ui` — PASS (1446)
- `cargo nextest run -p vtcode` feature filters
  (settings/model/palette/plugin/permission/limit/subagent/memory/plan/skills) — PASS (390–722 per filter; 2942 on the
  broader run)
- Design-kit unit tests (`design::list`, `design::keys`) — PASS
- Independent review + re-review — no remaining criticals

**Journey log** —

1. Protocol types stay in `vtcode-commons`; only factories/layout/keys belong in `vtcode-ui` — that dependency direction
   is load-bearing.
2. Mechanical `InlineListItem` → builder conversion needs struct-aware parsing; regexes that match `Some("lit")` also
   hit match patterns (`Some("run")`) — always constrain to struct-literal fields.
3. `action()` arity: description is `impl Into<String>`, badge is `Option<String>` — mixed `&str`/`String` call sites
   need one shape, not three.
4. Imports used only in `#[cfg(test)]` must live inside the test module or `-D unused-imports` fails the non-test build.
5. Residual raw literals are mostly dynamic/conditional rows (permission session/deny, url_guard, request_user_input) —
   safe follow-up polish, not behavior risk.

## [S1] Problem

Shared list UI is split across three places: protocol types in `vtcode-commons::ui_protocol`, rendering in `vtcode-ui`
modal code, and **row construction scattered** through `src/agent/runloop` (settings `action_item`/`section_item`, model
picker literals, plugins/skills menus, permission prompts, interactive_list). Group rhythm, value columns, badges,
keyboard copy, and focus/exit verbs have been unified piecemeal but are not one API, so each new modal re-implements
them and drifts.

`vtcode-ui::design` already owns color, constants, layout, and panel — it is the natural home for the **canonical list
design system**, while `InlineListItem`/`InlineTone` stay in `vtcode-commons` as the wire protocol.

## [S2] Design

### Decisions

1. **Protocol stays in commons** — `InlineListItem`, `InlineItemKind`, `InlineTone`, `InlineStatus` remain
   `vtcode-commons::ui_protocol`. `vtcode-ui` depends on commons; builders live in `vtcode-ui::design::list` and return
   those types.
2. **`vtcode-ui::design::list` is the only row factory** for modal/command UI. Call sites stop assembling raw
   `InlineListItem { .. }` literals except tests that pin renderer behavior.
3. **One keyboard contract** (`design::keys`) for navigate / apply / adjust / filter / back / cancel. Footer hints and
   docs read from these constants.
4. **One group rhythm** (already in `modal_list_item_lines`): blank above + below `group_header`; `group_divider` is a
   full-width rule. No per-surface spacing hacks.
5. **Accessibility conventions**: headers/hints non-selectable; Esc verb is `back` in nested views and `close`/`cancel`
   at root; status strips use tone + text (never color alone); values never rely on color alone (On/Off text).

### Canonical API (`crates/codegen/vtcode-ui/src/design/list.rs`)

```rust
pub use vtcode_commons::ui_protocol::{InlineItemKind, InlineListItem, InlineListSelection, InlineTone};

/// Section header (blank spacing above + below).
pub fn group_header(title: impl Into<String>) -> InlineListItem;
/// Full-width rule between groups.
pub fn group_divider() -> InlineListItem;
/// Non-selectable dimmed note.
pub fn hint(text: impl Into<String>) -> InlineListItem;
/// Title + accent value + dimmed description (settings, model capabilities).
pub fn setting(title, value, description, selection) -> InlineListItem;
/// Imperative row (Back, Reset, Refresh, …) with explicit tone.
pub fn action(title, description, badge, tone, selection) -> InlineListItem;
/// Choice row in a list (reasoning levels, tiers) — description only.
pub fn choice(title, description, selection) -> InlineListItem;
/// Choice row marked live/current (tone `Current`).
pub fn current_choice(title, description, selection) -> InlineListItem;
```

`design::keys` (single copy of footer copy):

| Constant      | Text                                              |
| ------------- | ------------------------------------------------- |
| `NAVIGATE`    | `↑↓ select`                                       |
| `APPLY`       | `Enter apply`                                     |
| `ADJUST`      | `←→ value`                                        |
| `FILTER`      | `type to filter`                                  |
| `BACK`        | `Esc back`                                        |
| `CANCEL`      | `Esc cancel`                                      |
| `LIST_HINT`   | join of NAVIGATE · APPLY · ADJUST · FILTER · BACK |
| `CHOICE_HINT` | `↑↓/jk move · Enter confirm · Esc clear/cancel`   |

`design::layout` adds `VALUE_COL` (28) used by the modal value column.

### Interaction / focus contract (documented + enforced by shared hints)

- List modal: printable → filter (if search on); ↑↓/jk navigate; Enter/Space apply; ←→ adjust values; Esc clears filter
  then backs/closes.
- Nested config views: Esc = Back; root Esc = close.
- Wizard/follow-up pickers: Esc cancels picker unless filter non-empty (clear first); `← Back` row returns to step 1.
- Empty filter result: Enter is a no-op; status/summary shows `No matching options`.

### Audit + migration map

| Surface                                                | File(s)                                  | Migrate to                                                                |
| ------------------------------------------------------ | ---------------------------------------- | ------------------------------------------------------------------------- |
| Settings palette                                       | `settings_interactive/{render,items}.rs` | `list::{group_header, setting, action, hint}`                             |
| Model picker                                           | `model_picker/rendering*.rs`             | `list::{group_header, group_divider, setting, action, current_choice}`    |
| interactive_list                                       | `vtcode-ui/tui/ui/interactive_list`      | `design::keys::CHOICE_HINT`, `SearchCandidate` via list helpers if useful |
| Plugin / skill managers                                | `slash_commands/{plugins,skills}.rs`     | `list::{group_header, action, choice}`                                    |
| Agents / workspace / oauth / mcp / … slash menus       | `slash_commands/*`                       | `list::{group_header, action, choice}` where they build lists             |
| Permission / limit prompts                             | `tool_routing/*`                         | `list::{group_divider, action}`                                           |
| Sessions / theme / mode palettes                       | `unified/palettes.rs`                    | `list::{action, choice}`                                                  |
| Plan approval / start confirm                          | `planning_workflow/*`                    | `list::{action, choice}`                                                  |
| Inline event / request_user_input / url_guard / memory | various                                  | `list::` builders                                                         |

Remove after migration: settings `section_item`/`action_item*` wrappers (re-export or delete), `divider_item` in
model_picker, duplicated footer string constants.

### Rendering (unchanged contracts)

`modal_list_item_lines` already implements value column, tone badges, group spacing, status strip. Design-system work
does not change wire types or hit-test geometry; row builders must set `kind`/`value`/`badge_tone` consistently so
rendering stays one path.

## [S3] Out of Scope

- New visual themes / palette tokens (theme registry stays as-is).
- Changing `ThreadEvent`, config schema, or tool contracts.
- Ratatui-only widgets outside list modals (transcript, sidebar) except keyboard hint reuse.
- Figma/product-design artifacts.

## Tasks

- [x] T1: `vtcode-ui::design::list` + `design::keys` (+ `VALUE_COL`) with unit tests for row kind/tone/value defaults —
      acceptance: crate exports the API; tests pin group_header kind, setting value, action tone (covers: S2)
- [x] T2: Migrate settings palette + model picker onto `design::list` and `design::keys` — acceptance: no raw
      `InlineListItem {` literals in those modules except tests; existing settings/model tests pass (covers: S2;
      depends: T1)
- [x] T3: Migrate slash-command menus (plugins, skills, agents, workspace, oauth, mcp, secrets, …) + palettes +
      permission/limit + plan/inline/request_user_input onto `design::list` — acceptance: compile; targeted tests for
      those flows pass (covers: S2; depends: T1)
- [x] T4: Unify interactive_list hints/keys with `design::keys`; delete dead local builders/footer constants —
      acceptance: one source for keyboard copy; rg shows no duplicate nav hint strings outside `design::keys` + tests
      (covers: S2; depends: T2, T3)
- [x] T5: Docs (`docs/development/tui-design-system.md`) + full verification — acceptance: `./scripts/check-dev.sh`
      green; `cargo nextest run -p vtcode-ui` green; settings/model/slash tests green (covers: S2)
