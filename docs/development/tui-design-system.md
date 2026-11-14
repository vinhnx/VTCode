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

## Code surface

Fenced code is a first-class surface, not prose:

- **Primary** — plain code uses the theme foreground at full intensity; a block with a matching grammar uses the
  syntax-theme token colors. The default for agent-facing content.
- **Muted** — secondary surfaces (evidence behind a disclosure): the code foreground is receded toward the theme
  background and the line-number gutter stays neutral chrome. Syntax highlighting is preserved, so the tokens stay
  distinguishable while the block recedes behind its summary.

Chrome and code that must recede without becoming unreadable stop at the theme's contrast floor: the color moves
toward the background only as far as the minimum contrast ratio allows (`theme::recede_toward_background`). Never use
a faint/dim text attribute for this — several terminals attenuate it to near-invisible, and ratatui only ever *inserts*
modifiers.

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

## Terminal capability ladder

`vtcode_commons::terminal_capabilities` owns the single answer to "what can this terminal draw?". Every surface reads
that verdict instead of re-deriving it from environment variables, so the CLI and the TUI can never disagree about the
same machine. The probe runs once per process (`box_glyph_tier()`), so bordered chrome costs no per-frame environment
reads.

The ladder is deterministic:

| Tier          | Evidence                                                                                | Border set                      |
| ------------- | --------------------------------------------------------------------------------------- | ------------------------------- |
| `Ascii`       | `VTCODE_NO_UNICODE` is affirmative, or neither `TERM` nor a UTF-8 locale says otherwise | `+` `-` `\|`                    |
| `Unicode`     | `TERM` advertises Unicode, or `LANG`/`LC_ALL`/`LC_CTYPE` declares UTF-8                 | rounded `╭` `─` `╮` `\|`        |
| `RichUnicode` | the terminal program is known to render the dashed variants correctly                   | heavy double dashed `┏` `╍` `┓` |

- `TERM` is authoritative because it describes the terminal program. `TERM_PROGRAM` identifies the program itself, which
  is what selects the rich tier: iTerm2, WezTerm, and Hyper all advertise `TERM=xterm-256color`, so a `TERM`-substring
  allowlist never matched them.
- The env opt-outs follow the repository's canonical flag semantics (`vtcode_commons::utils::env_flag_enabled`), so only
  `1`, `true`, `yes`, `on`, or `debug` turn a flag on. `VTCODE_NO_UNICODE=0` leaves Unicode chrome enabled.
- Ratatui's `BorderType::Plain` still draws `┌─┐│└┘`, so the ASCII rung needs its own set
  (`terminal_capabilities::ASCII_BORDER_SET`). Chrome that picks its own Unicode style must degrade through
  `border_set_with_fallback` rather than hard-coding a border type.

## Copy contract

Chrome is not content. A copy carries the user's text, never the TUI's markers:

- The modal list's selection cursor `┃` is chrome. It is the only glyph VT Code paints as a selection indicator, so
  `MouseSelectionState::extract_text` drops the gutter column that carries it (and the blank cells aligned with it) for
  overlay selections.
- The transcript's `│` hierarchy markers are content and stay. They are indistinguishable from a panel frame by glyph
  alone, so VT Code never guesses; a terminal-side cell-border protocol would remove that ambiguity, but no shipping
  terminal implements one.

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
