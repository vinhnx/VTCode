# Interface and Session Settings

_Part of [VT Code Configuration](./config.md)._

## Settings palette and reset

In an interactive session, `/config` opens a concise root with seven curated groups: Model & Provider, Agent &
Automation, Approvals & Security, Tools & Integrations, Context & Memory, Interface & Terminal, and Performance &
Diagnostics. Each group shows its purpose and editable-setting count, then opens only commonly used fields. The
**Advanced settings** entry is a search-first index of the complete documented schema; search matches config paths,
labels, descriptions, current values, and options. Use `/config <path>` to open a group, the Advanced index, or a
specific nested field directly; nested selections are restored when returning to a parent view.

The palette header shows the current write target and configuration source; individual setting rows keep their current
value and description.

In VT Code Desktop, `/config` opens Settings → VT Code. **Browse all settings** opens the same catalog with the CLI's
typed defaults, curated groups, field labels, descriptions, and documented choices; optional documented fields can be
set before they have an explicit value. Documented choices are validated by both the editor and shared writer.
Credential-bearing fields remain hidden, and schema templates without a concrete key are read-only references.
The Provider section offers an in-app API-key form for static-key providers; login-flow providers still authenticate in the Terminal.

The palette's **Reset configuration** action shows the exact target file and requires confirmation. `/config reset`
opens the same confirmation view. A reset clears only that target layer by replacing its contents with an empty TOML
document. Lower-precedence layers and credentials are preserved.

Saving a setting updates only the selected field in the target layer; it does not flatten the effective configuration
into that file. Custom-provider and provider endpoint/credential fields are saved to the canonical user config unless an
explicit `--config` file was selected, so trusted provider settings cannot be copied into repository-controlled
configuration.

The CLI exposes the same service for non-interactive repair and cleanup:

```bash
# Clear the active workspace layer (or the explicit --config file)
vtcode config reset

# Clear the canonical user or current project-profile layer
vtcode config reset --global
vtcode config reset --project

# Select an explicit workspace-layer file for this invocation
vtcode --config ./nightly.toml config reset
```

`--global` and `--project` are mutually exclusive. The CLI prints the resolved file and does not remove secure
credentials or any other layer.

## Live reload

While a session is running, changes to watched configuration files are debounced and reloaded through the normal layered
configuration service. Safe runtime settings such as UI appearance and theme, status-line behavior, permissions, sandbox
and timeout policy, MCP approval policy, and custom provider definitions are applied without restarting. A
provider/model identity selected for the current session remains stable until a later turn or session when changing it
would invalidate the active client.

If a watched edit is malformed, inaccessible, or fails validation, VT Code keeps the last valid configuration and
displays a warning. Fixing the file causes the next debounced reload to apply it. File creation and deletion are also
observed, including user/project layers and an explicit `--config` file.

The settings palette is refreshed from a successful reload while retaining its current section and selected entry; if an
entry disappeared, selection falls back to the first available item.

## Feature flags

Optional and experimental capabilities are toggled via the `[features]` table in `vtcode.toml`. These allow you to
customize the behavior of various VT Code features.

```toml
[features]
streaming = true           # enable streaming responses
human_in_the_loop = true   # enable human-in-the-loop tool approval
participant_context = true # include participant context in messages
terminal_integration = true # enable terminal integration features
```

Supported features:

| Key                    | Default | Description                                |
| ---------------------- | :-----: | ------------------------------------------ |
| `streaming`            |  true   | Enable streaming responses in the UI       |
| `human_in_the_loop`    |  true   | Enable tool approval prompts               |
| `participant_context`  |  true   | Include participant context in messages    |
| `terminal_integration` |  true   | Enable terminal integration features       |
| `mcp_enabled`          |  false  | Enable Model Context Protocol integrations |

## External editor

Use `tools.editor` to control the external editor flow used by `/edit`, empty-prompt `Ctrl+E`, and single-click file
links in the TUI.

```toml
[tools.editor]
enabled = true
preferred_editor = ""
suspend_tui = true
```

For real file opens, VT Code launches GUI editors immediately and returns to the session without waiting, including when
an agent turn is active. Transcript and modal file links use an out-of-band bounded request queue; they do not submit
`/edit` input. VS Code reuses the current window when supported and preserves line/column targets. Duplicate pending
requests for the same target are coalesced.

If the selected editor is terminal-based (for example `vim`/`nvim`) and `suspend_tui = true`, VT Code suspends the TUI
and waits for the editor to close through the serialized terminal-editor path. Temporary-file `/edit` flows still wait
so VT Code can read edited content back into the composer. When a launch waits for edited content, VT Code injects the
editor's wait flag if the configured command omitted it (`code` → `code --wait`, `zed`/`subl`/`mate` → `--wait`,
`open -a …` → `open -W`); non-waiting real-file opens strip those flags.

### Interactive controls

You can manage this feature without editing TOML directly:

- `/config` → **Tools & Integrations** contains the `External Editor` setting.
- `/config tools.editor` opens the dedicated editor setup wizard directly.
- The guided flow can also take you to `/config file_opener` when you want to tune ANSI hyperlink URI handling
  separately.

For full editor detection, launcher behavior, and examples, see
[External Editor Configuration](../tools/EDITOR_CONFIG.md).

## Fullscreen interaction

When VT Code is using alternate-screen rendering, you can tune fullscreen-specific mouse and transcript behavior with
the `ui.fullscreen` table.

```toml
[ui.fullscreen]
mouse_capture = true
copy_on_select = true
scroll_speed = 3
```

- `mouse_capture` keeps mouse events inside VT Code for click-to-expand, click-to-position, link activation, and wheel
  scrolling. Set it to `false` when you want the terminal's native text selection while keeping fullscreen rendering.
- `copy_on_select` controls whether text selected inside VT Code is copied automatically when the mouse selection ends
  (click-drag or double-click word select). Set it to `false` to copy manually instead: `Ctrl+C` copies a transcript or
  composer selection, and `Ctrl+O` copies the last agent response. The option is also editable from `/config` →
  **Interface & Terminal**.
- `scroll_speed` multiplies mouse-wheel scrolling from `1` to `20`. It only affects wheel accumulation; page-based
  keyboard navigation is unchanged.

VT Code also honors these environment variables for default fullscreen behavior:

- `VTCODE_FULLSCREEN_MOUSE_CAPTURE=0|1`
- `VTCODE_FULLSCREEN_COPY_ON_SELECT=0|1`
- `VTCODE_FULLSCREEN_SCROLL_SPEED=<1-20>`

Transcript Review uses the same fullscreen rendering surface:

- The configured `open_transcript_review` binding (default `Ctrl+T`) opens or closes a session-local whole-conversation
  review with search, paging, complete copy, and export controls, including from inline mode.
- The configured `toggle_transcript_render_mode` binding (default `R`) toggles rich rendering and ANSI-free raw
  rendering.
- Compact successful command rows are contiguous-only; their styled shortcut and `click to expand` suffix is clickable
  when mouse capture is enabled and focuses the first capture in a group.
- `[` hands the complete conversation to native terminal scrollback until you return.
- `v` opens the complete conversation in your configured editor.
- The title's `[close]` control and the footer shortcut guide are mouse/keyboard affordances for the review panel.
- `open_transcript_review` and `toggle_transcript_render_mode` can be rebound through the existing keybinding
  configuration.
- `Alt+O` remains a compatibility alias. If the review action is unbound, `Ctrl+T` remains readline transpose.
- The `focus_input` binding (default `Alt+I`) dismisses palettes, pickers, the review viewer, and modal search states
  and returns to the composer input. Unlike `Esc`, it never interrupts a running turn or clears input, and it leaves
  edit-approval diff previews open since those require an explicit decision.
- The review hint uses the primary `open_transcript_review` binding and is omitted when that action is unbound.

The compact review UX can be configured independently:

```toml
[ui.transcript_review]
show_hints = true
show_shortcut_guide = true
show_close_button = true

[ui.keybindings]
open_transcript_review = ["ctrl+t"]
toggle_transcript_render_mode = ["r"]
focus_input = ["alt+i"]
```

All three review controls default to enabled, while `ui.tool_display_mode` defaults to `"compact"`. These settings
affect only presentation; complete captures and raw exports remain unchanged.

## Diff preview layout

Interactive file-edit approval overlays and tool-output diffs support two layouts via `ui.diff_preview_mode`:

```toml
[ui]
# "inline" (default): unified diff; the value is retained for compatibility
# "side-by-side": old pane on the left, new pane on the right
diff_preview_mode = "side-by-side"
```

Side-by-side falls back to the unified view when the terminal is too narrow for two usable panes. The mode applies to
both the file-edit approval overlay and expanded tool-output diffs in the transcript (write_file, apply_patch, and
detected `git diff` shell output).

Large previews remain inspectable: VT Code bounds rendered rows with a head/tail omission marker and can skip expensive
syntax or intraline work without suppressing the underlying diff. In a preview overlay, use Up/Down or PageUp/PageDown
to scroll and Tab/Shift-Tab to jump between hunks.

For the full shortcut list and tmux notes, see [Keyboard Shortcuts](../user-guide/keyboard-shortcuts.md) and
[Interactive Mode Reference](../user-guide/interactive-mode.md).
