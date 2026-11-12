<!-- markdownlint-disable MD013 -->

# Desktop Embedded Terminal Guide

The desktop app embeds one login-shell terminal per workspace in a bottom panel. It is a user shell, not an
agent tool: keystrokes go to your `$SHELL -l`, output renders in the panel, and nothing crosses into the agent
runloop, the harness event stream, or `ThreadEvent`.

## Open, close, resize

- Toggle: `Cmd-J` (fixed binding), the View menu, or the Terminal button in the navigation rail.
  Settings → Connections offers a Run button that opens the panel and runs the session fork
  command for the selected thread.
- Dock position: Settings → Appearance → Terminal position docks the panel at the bottom
  (default), left, or right of the content. Side panels span the content height and resize
  by dragging their inner edge; the collapse chevron points outward per side. Grid rows and
  columns always follow the measured panel geometry.
  With no workspace open, the shell starts in your home directory. The panel keeps its session while
  the workspace is open; switching between workspaces closes the old session and its process group,
  while opening the first workspace keeps a home shell running.
- Resize: drag the panel edge (top for bottom, inner for sides), or focus the edge and use the arrow
  keys. Height persists per install (72–600px); side width persists too (240–640px). Grid rows
  recompute from the measured row height and columns from the laid-out panel width, so the shell
  always sees the true viewport size. The panel floats as a card on the window chrome with the
  workspace rhythm (12px gaps, shared spines); zen mode docks it flush.
- The header tab shows the shell's directory (`~` for home) with a close button; the chevron at the
  right collapses the panel. Both hide the panel without killing the session. A Terminal row in the
  Utilities pane opens the panel from the inspector.

## Input and paste

- Typing, navigation keys, `F1`–`F12`, and an allowlisted `Ctrl` subset reach the shell. Application-cursor
  mode is honored for arrow keys.
- `Cmd-V` pastes through bracketed paste when the shell supports it. Large pastes are chunked so the PTY
  never drops bytes; normal typing is never paced.
- Mouse wheel scrolls history; inside full-screen apps (vim, less) the wheel becomes arrow keys instead.

## Selection and copy

- `Cmd-A` selects the visible viewport; `Cmd-C` copies the selection; bare `Escape` clears the selection
  (the shell still receives the key).
- Drag with the left button to select an arbitrary span, in either direction; a plain click clears the
  selection. Releasing outside the panel ends the drag without clearing. Double-click selects the
  word under the cursor (or the bracketed span on a bracket); triple-click selects the whole line.
  Shift-click extends the current selection from its anchor in either direction.
- Typing or pasting snaps a scrolled viewport back to live output.

## Scrollback

- 10,000 lines of history. `Shift-PageUp`/`Shift-PageDown` move a page, `Shift-Home`/`Shift-End` jump to
  top/live. Plain navigation keys always reach the shell.

## Type

- The terminal renders in the desktop code font at the code font size (Settings → Appearance →
  Typography), and follows size changes live. Text rendering, including antialiasing and macOS
  font smoothing, is the same pipeline as the rest of the app — there is no terminal-specific
  text tuning to configure.

## Colors

- Default foreground/background/caret follow the desktop theme; the 16 ANSI colors default to the
  fixed Ciapre colorscheme (WCAG AA 4.5:1 on the Ciapre ground).
- Settings → Appearance → Terminal colors is a swatch grid (ground plus red/green/blue dots per
  scheme): `Ciapre` (default), `Auto` (follow the desktop mode with Rio's dark table or a
  desktop-owned light table, AA on every built-in light theme), `Dark`, `Light`, or `Latte` (fixed
  Catppuccin Latte colorscheme, AA on the latte ground). The live session re-renders immediately;
  theme and contrast changes re-sync automatically. Ciapre is a dark-ground scheme: on light desktop
  themes the base text stays readable (it tracks the chrome) but SGR-colored output reads dim —
  switch to `Auto`, `Light`, or `Latte` there, as with `Dark`.
- Default-background cells render transparent, so the grid always matches the panel surface no matter
  how stale the palette is; only explicit SGR backgrounds (and inverse/cursor/selection swaps) paint.

## Security model

Child output is untrusted. The panel enforces an output-trust boundary:

- OSC-52 clipboard writes and reads are denied: programs cannot touch your clipboard. Copy/paste only
  happen through your explicit `Cmd-C`/`Cmd-V`.
- Titles, bells, and graphics events have no UI surface and are dropped. The shell-reported working
  directory (OSC 7) shows in the tab label after control characters are stripped, so hostile
  output cannot break the single-line layout.
- Device, size, and color queries are answered back to the PTY only, never to the UI.
- Drain batches are bounded (64 PTY events, 256 view events, 10k scrollback) so hostile output (bell
  floods, unbounded lines) degrades to dropped events, not hangs or unbounded memory.
- Closing the panel kills the shell's foreground process group and reaps the shell. Detached background
  groups the shell disowned may survive; this limitation is documented on `close()`.
- The shell inherits the desktop environment, like any terminal emulator, and runs outside the agent
  sandbox: the [command security model](COMMAND_SECURITY_MODEL.md) gates agent-issued commands, not your
  keystrokes.

## Files

- `crates/codegen/vtcode-desktop/src/desktop/terminal_{process,screen,tab}.rs`: PTY spawn/drain,
  Rio grid wrapper, retained session.
- `crates/codegen/vtcode-desktop/src/desktop/view/terminal_panel.rs`: toggle, pump, rows, input,
  selection, wheel, palette sync.
- `crates/codegen/vtcode-desktop/src/desktop/preferences/terminal.rs`: height bounds, ANSI setting.
- `crates/codegen/vtcode-desktop/src/desktop/themes/terminal.rs`: fixed light ANSI table (hex lives
  under `themes/` by the desktop hard rule).
