# GPUI-Kit message scroller: tail-follow pause

Vendored `gpui-component` 0.9.1 (Apache-2.0), re-exported through
`gpui-kit` 0.9.1 (`gpui_kit::component`). Original copyright notices and
`LICENSE-APACHE` are retained.

Changes are in `src/message_scroller.rs`:

- `MessageScrollerState::pause_following_tail()` — exposes gpui's
  `ListState::pause_following_tail` (the inner `ListState` field is
  private, so downstream crates could not reach it).

And in `src/sidebar/menu.rs`:

- `SidebarMenuItem` rows now set a pointing-hand cursor when clickable and
  keep the default arrow when disabled. Kit's Settings sidebar renders
  its selectable section rows as `SidebarMenuItem`s, which previously
  showed the arrow cursor on hover unlike the app's `Button`-based rows.

And in `src/tab/tab.rs`:

- `Tab` rows now set a pointing-hand cursor when clickable (enabled with a
  click handler) and keep the default arrow otherwise. VT Code's thread
  tabs, attachment-picker tabs, and inspector tabs select via `TabBar`'s
  `on_click`, so they previously hovered with the arrow.

And in `src/select.rs` (dropdown trigger contract):

- The popup anchors to the trigger pill itself: `SelectState` captures the
  `#input` trigger bounds at prepaint (`trigger_bounds`) and uses them for
  the `dropdown_popup` anchor, falling back to the wrapper bounds. With the
  default `Length::Auto` menu width the menu is `min_w(trigger_width)` —
  never narrower than the pill, content-driven beyond — instead of
  stretching to the wrapper (which filled its parent and rendered a
  full-window-wide menu).
- The trigger's outline/ring only shows for keyboard focus while closed:
  an open menu is its own state indicator, so `outline_visible` excludes
  the `open` state.

And in `src/button/button.rs` (same contract, Button side):

- `shows_selected_style()` is `self.selected` only: `Selectable::open`
  keeps storing/reading open state (a11y, call-site tests) but no longer
  paints the selected/pressed fill, so dropdown trigger pills stay quiet
  while their popup is up.

VT Code's transcript pauses tail-following when the reader interacts with
a row (expanding a tool call or diff) while the runloop streams. The
pause-only operation freezes the viewport at its exact item and pixel
offset; the previous workaround re-anchored with `scroll_to_item`, which
dropped the pixel offset (viewport shifted by up to a row height) and
stranded short transcripts at their top row.

Upstream request to carry this in the next release: add a pause-only
method to `MessageScrollerState`; when published, drop this vendored
patch and bump `gpui-kit`.

Re-vendor procedure (per `=0.9.1` bump): copy the new registry source
over `src/`, drop `Cargo.lock` and `Cargo.toml.orig`, re-apply the
`pause_following_tail` addition, the `SidebarMenuItem` / `Tab` cursor
changes, the `Select` trigger anchoring/width/outline contract, and the
`Button` open/selected paint separation; then diff `src/message_scroller.rs`,
`src/sidebar/menu.rs`, `src/tab/tab.rs`, `src/select.rs`, and
`src/button/button.rs` against the registry copy to confirm the deltas
contain only these additions (`diff -u` each file). See
`.vtcode/memory/gotchas.md` for the general re-vendor checklist.
