# Keyboard Shortcuts

Single source of truth for VT Code terminal keyboard shortcuts. Press `?` on an empty input line while VT Code is
running to open the in-app shortcut overlay (mirrors this guide).

> Platform notes:
>
> - `Cmd` means `Super`/`Meta` (`KeyModifiers::SUPER | META`): macOS Command, Windows Super, Linux Meta/Super. It is not
>   `Ctrl`.
> - **Unified across terminals**: terminals using the Kitty keyboard protocol report `Cmd+Left/Right/Backspace` as
>   `SUPER`-modified keys. Legacy macOS terminals (Terminal.app, iTerm2, VS Code, older emulators) instead map them to
>   C0 control codes — `Cmd+Left` → `0x01` (`Ctrl+A`), `Cmd+Right` → `0x05` (`Ctrl+E`), `Cmd+Backspace` → `0x15`
>   (`Ctrl+U`), `Cmd+Delete` → `0x0B` (`Ctrl+K`). VT Code folds both families into the same line-wise handlers, so the
>   composer behaves identically either way. Because the legacy encoding cannot distinguish `Ctrl+A` from `Cmd+Left`,
>   `Ctrl+A`/`Ctrl+E` are line-wise and `Ctrl+U` clears the current line by design.
> - Terminals report `Shift+Tab` as `BackTab` (sometimes with the `SHIFT` bit); some report `Tab+SHIFT` or
>   `Char('\t')+SHIFT`. All three cycle agents. Plain `Tab` never cycles — it enqueues.
> - Some terminals deliver `Tab` as `Char('\t')` and `Ctrl+Enter` as forked sequences; behavior is identical in both
>   encodings.
> - `Esc Esc` must be consecutive presses within ~800ms; any other key disarms the timer.

## Composer submit and queue

| Shortcut                                             | Idle                                                       | Active turn / busy                                                                                                                    |
| :--------------------------------------------------- | :--------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------ |
| `Enter`                                              | Submit draft.                                              | Steer plain text into the active turn; slash commands keep queue routing.                                                             |
| `Ctrl+Enter` or `Tab`                                | Submit draft; empty draft processes newest queued message. | Queue draft (`QueueSubmit`); plain text is batchable, slash stays one-per-turn. App handles `/stop`, `/pause`, `/resume` immediately. |
| `Tab` (first)                                        | Accept visible ghost suggestion.                           | Same accept-first priority.                                                                                                           |
| `Shift+Enter` / `Alt+Enter` / `\`+`Enter` / `Ctrl+J` | Insert newline.                                            | Same.                                                                                                                                 |
| `Enter` on empty draft with active jobs              | Submit `/config jobs`.                                     | Same.                                                                                                                                 |

Palettes consume `Tab`/`Enter`/`Esc` first: agent/file palettes select best match on `Tab`, slash navigation
autocompletes on `Tab`, history picker consumes navigation keys.

## Composer line editing

| Shortcut                                             | Single-line input                                                                     | Multiline input                                                                                                                                                                                     |
| :--------------------------------------------------- | :------------------------------------------------------------------------------------ | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Cmd+A` or `Cmd+Backspace`                           | Clear entire input, attachments, and compact state atomically.                        | Clear only the cursor's logical line; a collapsed `[Pasted Content N chars]` block overlapping the line or cursor is removed atomically. Image `[Image #N]` tokens on the line are removed with it. |
| `Cmd+Left` / `Cmd+Right` (`Shift` extends selection) | Buffer start/end.                                                                     | Current logical-line start/end (logical `\n` lines, not visual wraps).                                                                                                                              |
| `Esc Esc` (focused composer)                         | Content present: clear entire input. Empty input: open the rewind picker (`/rewind`). | Content present: clear the current logical line. Empty input: open the rewind picker. First press arms, second acts.                                                                                |
| `Cmd+E`                                              | Line end.                                                                             | Current line end.                                                                                                                                                                                   |

On terminals that send the legacy control-code aliases, the same behavior is reachable without the Kitty protocol:
`Ctrl+A`/`Ctrl+E` move to the current line's edges and `Ctrl+U` clears the current line (see the readline table below).
No extra terminal setup is needed beyond the terminal's own Cmd mapping. Secure-prompt (single-line secret) mirrors
this: `Cmd+Backspace`/`Cmd+A` clear, `Cmd+Left/Right` jump to edges, `Esc` cancels immediately without double-press.

## Readline and word editing

| Shortcut                             | Action                                                                                                                                                                                |
| :----------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `Ctrl+A` / `Ctrl+E`                  | Current line start / end (buffer edges when single-line; legacy `Cmd+Left`/`Cmd+Right`).                                                                                              |
| `Ctrl+F` / `Ctrl+B`                  | `Ctrl+F` moves forward. `Ctrl+B` hands an active foreground command to the background session manager; when no command is active, it runs the configured background-operation action. |
| `Alt+F` / `Alt+B`, `Alt+Left/Right`  | Word forward / back.                                                                                                                                                                  |
| `Ctrl+P` / `Ctrl+N`, `Up`/`Down`     | History previous / next (`Up`/`Down` stay inside a multi-row composer and never traverse history; `Ctrl+P`/`Ctrl+N` always traverse history).                                         |
| `Ctrl+W`, `Alt+D`                    | Delete previous / next word.                                                                                                                                                          |
| `Ctrl+U` / `Ctrl+K`                  | Clear current line / delete to line end (legacy `Cmd+Backspace`/`Cmd+Delete`).                                                                                                        |
| `Ctrl+T`                             | Transpose chars (when Transcript Review is unbound; otherwise opens review).                                                                                                          |
| `Alt+T`                              | Transpose words / toggle tool summaries per binding.                                                                                                                                  |
| `Alt+U` / `Alt+L` / `Alt+C`, `Alt+\` | Uppercase / lowercase / capitalize word, delete whitespace around cursor.                                                                                                             |
| `Ctrl+Z` / `Ctrl+Y`                  | Undo / redo.                                                                                                                                                                          |
| `Ctrl+G`                             | Open the external editor: with the composer draft, or the persisted plan file from the "Ready to code?" approval overlay (saved edits re-show the overlay).                           |
| `Home` / `End` (`Shift` selects)     | Buffer start / end.                                                                                                                                                                   |

## Agent and mode switching

| Shortcut                                                 | Action                                                                                                      |
| :------------------------------------------------------- | :---------------------------------------------------------------------------------------------------------- |
| `Shift+Tab` (`Tab+SHIFT`, `BackTab`, `Char('\t')+SHIFT`) | Cycle primary agent forward (`CyclePrimaryAgent`); `BackTab` cycles previous (`CyclePrimaryAgentPrevious`). |
| Locked (`Building`, `Recovery`, `Blocked`, busy handoff) | Switch dropped with mode-switch notice; `Tab` enqueue still works.                                          |

`Ctrl+B` is context-sensitive and takes precedence over composer editing while a foreground PTY or pipe command is
running. The handoff keeps the process alive and returns a reusable session id; only three live background processes may
be retained per VT Code runtime. A modal that owns input before action dispatch keeps its own higher-priority handling.

## Overlays, palettes, and lists

| Context                            | Keys                                                                                                                                                                                                                                                                                                                                                            |
| :--------------------------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Agent/file palette                 | `Up`/`Down` move, `Tab` select best match, `Enter` open/apply, `Esc` close. In the `@` file picker, `PageUp`/`PageDown` jump a page, `←` ascends a directory, and `Alt+Enter` references the highlighted folder (`@dir/`) instead of opening it.                                                                                                                |
| Slash palette                      | `Tab` autocomplete, navigation via slash keys.                                                                                                                                                                                                                                                                                                                  |
| History picker (`Ctrl+R`/`Ctrl+S`) | Type to filter, `Tab`/`Esc`/`Enter` accept, `Ctrl+C`/`Ctrl+G` or empty `Backspace` cancel; Ctrl-modified characters are always shortcuts, never search text.                                                                                                                                                                                                    |
| Modal list                         | `Tab`/`BackTab` move per modal, `Esc`/`Enter` cancel/submit per overlay. Modals and multi-step wizards without a search box number their options when nine or fewer are visible: `1`–`9` jumps to an option (`Enter` still confirms, except wizards confirm immediately). In numbered modals, out-of-range digits are swallowed, never typed into the composer. |
| Local agents window                | `Down` opens on empty composer; `Alt+S` focuses. `Enter` inspects; `Ctrl+K` stops; `Ctrl+X` force-terminates or closes. For `exec-session` rows, `Ctrl+R` toggles stdin focus and `Ctrl+P` previews the bounded snapshot.                                                                                                                                       |
| Queued-input edit                  | `Alt+Up` (or `Shift+Left` in tmux) pops newest queued message into composer.                                                                                                                                                                                                                                                                                    |

With the composer focused, Shift/Alt+Enter inserts a newline before command dispatch, including `/stop`, `/plan`, and
`/help` while a turn runs. Modified Enter preserves a trailing backslash; plain backslash+Enter consumes it.
Overlay bindings still take precedence.

## Transcript Review and fullscreen

| Shortcut                                                                | Action                                            |
| :---------------------------------------------------------------------- | :------------------------------------------------ |
| `Ctrl+T` (default) / `Alt+O`                                            | Open/close Transcript Review.                     |
| `r`                                                                     | Toggle rich/raw.                                  |
| `/`, `Enter`, `Esc`, `n`/`N`                                            | Search start/commit/cancel, next/previous match.  |
| `j`/`k`, `Up`/`Down`, `Ctrl+U`/`D`, `Ctrl+B`/`F`, `g`/`G`, `Home`/`End` | Scroll line, half-page, full-page, top/bottom.    |
| `v`, `[`, `q`                                                           | Open in editor, hand to native scrollback, close. |
| `PgUp`/`PgDn`, wheel, `Ctrl+Home`/`End`                                 | Fullscreen transcript scroll.                     |

Screen-reader mode opens review in raw mode and switches an open review to raw when enabled. Manual rich/raw toggling
remains available; disabling the setting leaves the current mode unchanged.

## Multiline input methods

| Method            | Shortcut                                                                                                                                                                                                                                                                                           |
| :---------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Quick escape      | `\` + `Enter`                                                                                                                                                                                                                                                                                      |
| macOS default     | `Option+Enter`                                                                                                                                                                                                                                                                                     |
| Native/configured | `Shift+Enter` (Ghostty, Kitty, WezTerm, iTerm2, Warp natively; `/terminal-setup` for VS Code, Alacritty, Zed)                                                                                                                                                                                      |
| Control sequence  | `Ctrl+J`                                                                                                                                                                                                                                                                                           |
| Paste             | Paste directly (large inputs collapse to `[Pasted Content N chars, M lines, K images, J files]` once text reaches 10+ lines, 1000+ chars, 3+ images, or 5+ `@file` tokens; shows head + tail snippets, click or paste again to expand for editing; one `Backspace` at block end removes the block) |
| `Shift+Ctrl+V`    | Paste clipboard text as raw full content, bypassing the collapse marker entirely                                                                                                                                                                                                                   |

## Quick commands

| Input        | Action                                                                               |
| :----------- | :----------------------------------------------------------------------------------- |
| `#` at start | Custom prompts picker.                                                               |
| `/` at start | Slash command (`/help` lists all).                                                   |
| `!` at start | Bash mode (direct execution).                                                        |
| `@`          | File picker; `@agent-<name>` subagent picker (`@agent-<plugin>:<name>` for plugins). |
| `Alt+P`      | Ghost suggestion; `Tab` accepts.                                                     |

## Custom keybindings

Rebindable actions (`open_transcript_review`, `toggle_transcript_render_mode`, `toggle_tool_display_mode`,
`toggle_task_panel`, `scroll_page_up/down`, `jump_to_last_change`, `interrupt`, `exit`, history, queue edit) live under
`ui.keybindings` (`KeyBindingConfig::bindings` / `UserPreferences::keybindings`, `cmd`/`super` = Command). Composer
editing keys above (`Tab`, `Esc`, `Cmd+A`, arrows, readline) are intentionally hardcoded in `session/events.rs` and not
rebindable. See [Configuration](../config/config.md) and [Interactive Mode](./interactive-mode.md).

Cancellation stops the active task without discarding completed output or composer drafts. Press Ctrl+C again within
one second to exit; a fresh submission cannot override an accepted exit. Cancelled work requires fresh user input
to resume and never triggers automatic recovery or continuation. During initialization, cancellation pauses the
remaining setup work until a fresh submission. See [response progress](../development/response-progress.md).
