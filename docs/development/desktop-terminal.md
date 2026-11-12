<!-- markdownlint-disable MD013 -->

# Desktop Embedded Terminal Guide

The desktop app embeds one login-shell terminal per workspace in a bottom panel. It is a user shell, not an
agent tool: keystrokes go to your `$SHELL -l`, output renders in the panel, and nothing crosses into the agent
runloop, the harness event stream, or `ThreadEvent`.

## Open, close, resize

- Toggle: `Cmd-J` (fixed binding) or the View menu. The panel keeps its session while the workspace is open;
  switching workspaces closes the old session and its process group.
- Resize: drag the panel's top edge, or focus the edge and use the arrow keys. Height persists per install
  (72–600px); grid rows recompute from the measured row height and columns from the laid-out panel
  width, so the shell always sees the true viewport size.

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
  selection. Releasing outside the panel ends the drag without clearing.
- Typing or pasting snaps a scrolled viewport back to live output.

## Scrollback

- 10,000 lines of history. `Shift-PageUp`/`Shift-PageDown` move a page, `Shift-Home`/`Shift-End` jump to
  top/live. Plain navigation keys always reach the shell.

## Colors

- Default foreground/background/caret follow the desktop theme; the 16 ANSI colors default to Rio's dark
  table, or to a desktop-owned light table (WCAG AA 4.5:1 on every built-in light theme) when the desktop
  theme is light.
- Settings → Appearance → Terminal colors: `Auto` (follow the desktop mode), `Dark`, or `Light`. The live
  session re-renders immediately; theme and contrast changes re-sync automatically.

## Security model

Child output is untrusted. The panel enforces an output-trust boundary:

- OSC-52 clipboard writes and reads are denied: programs cannot touch your clipboard. Copy/paste only
  happen through your explicit `Cmd-C`/`Cmd-V`.
- Titles, working-directory reports, bells, and graphics events have no UI surface and are dropped.
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
