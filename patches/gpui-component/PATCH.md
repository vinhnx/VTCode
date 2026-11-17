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
  keep the default arrow when disabled. Kit's `Settings` sidebar renders
  its selectable section rows as `SidebarMenuItem`s, which previously
  showed the arrow cursor on hover unlike the app's `Button`-based rows.

And in `src/tab/tab.rs`:

- `Tab` rows now set a pointing-hand cursor when clickable (enabled with a
  click handler) and keep the default arrow otherwise. VT Code's thread
  tabs, attachment-picker tabs, and inspector tabs select via `TabBar`'s
  `on_click`, so they previously hovered with the arrow.

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
`pause_following_tail` addition and the `SidebarMenuItem` / `Tab` cursor
changes, and diff `src/message_scroller.rs`, `src/sidebar/menu.rs`, and
`src/tab/tab.rs` against the registry copy to confirm the deltas contain
only these additions (`diff -u` each file). See `.vtcode/memory/gotchas.md`
for the general re-vendor checklist.
