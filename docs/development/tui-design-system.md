# TUI design system

Canonical list UI lives in **`vtcode_ui::design`**. Wire types stay in `vtcode_commons::ui_protocol` (protocol layer
between the agent and the TUI).

## Modules

| Module                               | Role                                                                                                    |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| `design::list`                       | Row factories: `group_header`, `group_divider`, `hint`, `setting`, `action`, `choice`, `current_choice` |
| `design::keys`                       | Keyboard/interaction copy (`list_hint()`, `choice_hint()`)                                              |
| `design::constants`                  | Spacing tokens and `VALUE_COL` (value-column width)                                                     |
| `design::layout` / `panel` / `color` | Layout modes, panel chrome, color bridging                                                              |

## Row contract

- **Group header** — `» Title` (leading anchor), bold title, blank spacing above and below. Non-selectable.
- **Setting** — `title` + accent `value` + dimmed description. Tone via `badge_tone`.
- **Action** — imperative row with explicit tone (Neutral/Accent/Success/Warning/Danger).
- **Choice / current_choice** — list options; `current_choice` marks `Current`.
- **Hint** — dimmed note; never a header.
- **group_divider** — full-width rule between sections.
- **Density** — every selectable row keeps one blank separator row after it, in core modal lists and the standalone
  `run_interactive_selection` picker alike, so dense subtitle lists stay scannable.

Do **not** assemble `InlineListItem { .. }` literals in command/UI code; call the factories and tweak with
`with_search_value` / `with_badge` when needed.

## Interaction contract

- Filter-aware lists: printables type into the filter; Esc clears, then backs/closes.
- `↑↓ select · Enter apply · ←→ value · type to filter · Esc back` (`design::keys::list_hint`).
- Standalone pickers: `design::keys::choice_hint` (`↑↓/jk move · … · Esc clear/cancel`).
- Nested views: Esc = Back; root Esc = close. Empty filter result blocks Enter.

## Accessibility

- Status and badges always include text (not color alone).
- Headers and hints are non-selectable so screen readers and keyboard users are not trapped on chrome rows.
- Value column uses `constants::VALUE_COL` so scanning stays consistent.

## When adding a modal

1. Build rows with `design::list`.
2. Use `design::keys` for footer/hint copy.
3. Prefer `setting` for anything with a live value; `action` for commands.
4. Group long lists with `group_header` / `group_divider` — the renderer supplies spacing.

## Sticky prompt navigation

The normal transcript reserves one row for the user prompt owning its first visible content once that prompt's first
reflowed row has scrolled above the viewport. Consecutive User lines form one multiline prompt. Its single-line preview
uses the existing user prefix, collapsed whitespace, a Unicode-width ellipsis, and the input background and foreground
theme tokens (falling back to the theme background when the input tint would reduce contrast below AA). The composer
stays pinned. The header is hidden when the original first row is visible, no complete owning prompt remains in live
history, or fewer than four transcript rows are available. Archived-history pagination is outside this surface.

A plain left-click on the header jumps to the original prompt's first row, clears pending selection/link actions, and
pauses auto-follow unless the destination is already at the bottom. Overlays retain mouse ownership. Body rendering,
links, selection, queued input, and drag scrolling use the remaining body rectangle. Reading preserves the message
anchor during resize and streaming. At the live bottom, reserve the row before calculating the body origin; if this
exposes a new prompt's first row, yield the header for that position so repeated frames remain stable.

The UI-only rendered click target is invalidated on content or layout changes. `session/sticky_prompt.rs` owns prompt
lookup and layout; both mouse-event paths call its shared navigation helper. This follows the
[Codex prompt-header pattern](https://github.com/openai/codex/blob/7219fd735bef2f9cfd0363fecdbbb212e3df5255/codex-rs/tui/src/transcript_view.rs)
with VT Code's existing retained message indices and reflow cache.
