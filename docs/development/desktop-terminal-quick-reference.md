<!-- markdownlint-disable MD013 -->

# Desktop Embedded Terminal Quick Reference

One login shell per workspace, bottom panel. See the [full guide](desktop-terminal.md).

| Action | Input |
| --- | --- |
| Toggle panel | `Cmd-J`, View menu, rail Terminal button, or Utilities → Terminal |
| Hide panel | Tab `×` or header chevron (session keeps running) |
| Paste (bracketed) | `Cmd-V` |
| Select viewport | `Cmd-A` |
| Copy selection | `Cmd-C` |
| Clear selection | `Escape` (shell still receives it) |
| Drag-select / clear | Left-drag a span; plain click clears |
| Word / line select | Double-click a word; triple-click a line |
| Extend selection | Shift-click (from the selection anchor) |
| History page up/down | `Shift-PageUp` / `Shift-PageDown` |
| History top/live | `Shift-Home` / `Shift-End` |
| Wheel | Scroll history (arrows in full-screen apps) |
| Resize | Drag top edge; arrows when edge focused |

| Setting | Location | Values |
| --- | --- | --- |
| Terminal colors | Settings → Appearance | `Auto` (default), `Dark`, `Light` |
| Terminal position | Settings → Appearance | `Bottom` (default), `Left`, `Right` |

| Limit | Value |
| --- | --- |
| Scrollback | 10,000 lines |
| Panel height | 72–600px, persisted |
| Drain batch | 64 PTY events, 256 view events |
| Clipboard escape (OSC-52) | Denied both directions |
