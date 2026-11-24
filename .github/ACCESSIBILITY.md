# Accessibility Statement

VT Code is a keyboard-first terminal coding agent. We want it to be usable by everyone, including people who rely on
keyboard-only operation, reduced motion, high-contrast or light/dark terminal themes, and assistive technology.

Our practical target is WCAG 2.1 AA where it applies to a terminal app — notably 4.5:1 minimum text contrast — plus
no pointer-only workflows, honored OS accessibility preferences, and plain-text alternatives to the fullscreen TUI.
That is a target, not a verified conformance claim: we have not run a formal WCAG evaluation, so we document the
environments we test and the gaps we know about instead.

## What we do

### Keyboard operability

- The TUI is keyboard-first. Every workflow (compose, plan, review diffs, approve tools, manage sessions) is reachable
  by keyboard.
- Press `?` on an empty input line in the app to open the shortcut overlay. The canonical reference is
  [Keyboard Shortcuts](../docs/user-guide/keyboard-shortcuts.md).
- Optional Vim-style prompt editing (`ui.vim_mode`, `/vim on`) is available for users who prefer modal editing.
- Mouse handling is optional. Set `ui.fullscreen.mouse_capture = false` to return click-and-drag selection,
  wheel scrolling, and link activation to the terminal.

### Color and contrast

- Default minimum contrast is WCAG AA 4.5:1 (`ui.minimum_contrast = 4.5`). WCAG AAA 7.0 is supported via
  `minimum_contrast = 7.0`. See [Terminal Color Guidelines](../docs/guides/color_guidelines.md).
- `ui.high_contrast_mode = true` (or `VTCODE_HIGH_CONTRAST=1`) raises the effective floor to 7.0:1 in the TUI without
  rewriting a saved `minimum_contrast`. The desktop app does not read this setting; it has its own independent
  Settings → Accessibility → High contrast switch, which drives the palette to maximum contrast and restores the
  previous slider value when switched off.
- Theme contrast is validated at startup with warnings, and every built-in theme is gated to meet the AA floor
  (`cargo nextest run -p vtcode-ui -E 'test(theme)'`).
- `NO_COLOR=1` and `vtcode --no-color` suppress ANSI color; `ui.safe_colors_only` restricts output to the 11 portable
  ANSI colors; `ui.bold_is_bright` avoids bold styling for legacy terminals.
- Light/dark terminal detection (`ui.color_scheme_mode = "auto"`) follows Contour, OSC 11, `COLORFGBG`, and
  `TERM_PROGRAM` heuristics, including live light/dark follow where the terminal reports it.

### Reduced motion

- `ui.reduce_motion_mode = true` keeps progress labels visible as static text while stopping shimmer and spinner
  animation. Per-run: `VTCODE_REDUCE_MOTION=1 vtcode`.
- When unset, VT Code reads the OS accessibility preference where supported (Windows, macOS, GNOME/KDE/XFCE on Linux).
  An explicit `true`/`false` always wins. See [Interactive Mode](../docs/user-guide/interactive-mode.md#reduced-motion).

### Screen readers and assistive technology

- `ui.screen_reader_mode = true` (or `VTCODE_SCREEN_READER=1`) disables animations, uses plain-text indicators, labels
  diagnostic severity in text (`[Warning]`, `[Error]`), and optimizes output for assistive technology. It also forces the
  scrollback-friendly inline surface even when an explicit `agent.ui_surface` or Codex `tui.alternate_screen` asks for
  the alternate screen.
- Headless commands produce linear plain-text output that works better with screen readers and pipes:
  `vtcode ask`, `vtcode exec`, and `vtcode review`.
- Transcript Review (`Ctrl+T`) has an ANSI-free raw rendering mode (`R`), `Ctrl+O` to copy, `v` to open the full
  conversation in your editor, and `[` to hand the conversation to the terminal's native scrollback.
- Copying prefers native clipboard helpers (`pbcopy`, `xclip`/`xsel`/`wl-copy`, `clip.exe`) with OSC 52 fallback,
  and reports `Copy failed` instead of a false success.

## Supported environments

- Primary: macOS and Linux terminals. Windows artifacts are best-effort and may lag (see README).
- Terminals exercised in docs include Ghostty, Kitty, WezTerm, iTerm2, Warp, VS Code, Alacritty, and Zed; any
  standards-compliant terminal with keyboard input and text output should work, with graceful degradation where
  Kitty keyboard protocol, focus events, or live palette reports are unavailable.

## Known limitations

- The default interactive surface is inline, so output stays in terminal scrollback. Setting `agent.ui_surface =
  "alternate"` (or Codex `tui.alternate_screen = "always"`) switches to fullscreen rendering, which limits scrollback and
  some screen-reader virtual buffers; screen-reader mode overrides that choice back to inline. Workarounds for the
  fullscreen surface: headless `ask`/`exec`, Transcript Review raw mode (`R`), `[` for native scrollback, or `v` to read
  in your editor.
- Complex live-updating rows (progress, background-task indicators) are simplified under reduced-motion and
  screen-reader modes, but not every animated surface has a separate static equivalent yet.
- Windows support and some terminal-specific key bindings (for example `Shift+Enter` multiline input) vary by
  terminal; the keyboard-shortcuts guide documents per-terminal notes and fallbacks.

We treat these as bugs when they block real workflows. Reporting them helps us prioritize. Open remediation work is
tracked in [accessibility issues](https://github.com/vinhnx/VTCode/issues?q=is%3Aissue+is%3Aopen+a11y); closed items
in the same search show what has already been fixed.

## Reporting accessibility issues

1. Open an [accessibility barrier report](https://github.com/vinhnx/VTCode/issues/new?template=accessibility_report.md).
   It requests the `accessibility` and `bug` labels; GitHub applies only labels that already exist, so maintainers add
   `accessibility` during triage if it is missing. A [general bug report](https://github.com/vinhnx/VTCode/issues/new?template=bug_report.md)
   with `[a11y]` in the title works too.
2. Include whatever context you can: the task you were doing, the surface (TUI, headless command, desktop app, or
   docs), what you expected and what actually happened, your OS, terminal, and any assistive technology in use.
   Screenshots, recordings, and logs are optional — you do not need to disclose a disability or prove a barrier to
   report it.
3. If you already have a workaround, include it so we can share it while a fix is in progress.
4. For questions or design discussion, use
   [GitHub Discussions](https://github.com/vinhnx/VTCode/discussions).
5. Never report a security vulnerability in a public issue. Use
   [private vulnerability reporting](https://github.com/vinhnx/VTCode/security/advisories/new). Details:
   [Security Policy](../docs/SECURITY.md).

## How we respond

We acknowledge new reports, triage them alongside other bugs, and track confirmed barriers as `[a11y]` issues, adding
the `accessibility` label during triage. Maintainers assign or confirm severity during triage; reporters do not need
to. We post status updates when a fix is planned or blocked, note a workaround when one exists, and ask the reporter to
verify a fix when it ships. We do not promise a fixed timeline, and we keep reports open until the barrier is resolved
or clearly scoped out with a documented reason.

## Contributor expectations

- Keep new UI text at or above the configured minimum contrast (default 4.5:1); do not add pointer-only flows.
- Honor existing accessibility switches: `NO_COLOR`/`--no-color`, `reduce_motion_mode`, `screen_reader_mode`,
  `high_contrast_mode`, and `mouse_capture = false`. Animated or color-only signals need a static or textual equivalent.
- Test TUI changes with `VTCODE_REDUCE_MOTION=1`, `VTCODE_SCREEN_READER=1`, `VTCODE_HIGH_CONTRAST=1`, `NO_COLOR=1`, and
  a light and a dark theme before opening a PR, and run `./scripts/check-dev.sh` plus
  `cargo nextest run -p vtcode-ui -E 'test(theme)'`. `python3 scripts/check_accessibility.py` guards the documented
  switch contract, the reporting path, and desktop locale parity.
- The same expectations apply to coding agents. `AGENTS.md` carries the accessibility rules for automated changes,
  and agent-authored PRs are held to the same checks as human PRs.
- When a change affects prompts or terminal output, show the rendered result under the relevant switches
  (`VTCODE_SCREEN_READER=1`, `VTCODE_REDUCE_MOTION=1`, `NO_COLOR=1`, and a light and a dark theme) in the PR, not just
  a screenshot of the default theme.
- Follow [Contributing](../docs/CONTRIBUTING.md) (Conventional Commits, surgical diffs, docs with user-facing changes).

## Technical notes

VT Code relies on the terminal emulator plus the OS and any assistive technology installed on the machine. There is no
separate web runtime: conformance-relevant surfaces are plain-text terminal output, ANSI styling (with opt-outs above),
and Markdown documentation. No audio, video, or CAPTCHA content ships in the default TUI.

## Maintenance

This statement is maintained by the VT Code maintainers alongside the docs. Last reviewed: 2026-11-24. To suggest a
change to this statement or our accessibility practices, open an
[accessibility barrier report](https://github.com/vinhnx/VTCode/issues/new?template=accessibility_report.md) or start a
[discussion](https://github.com/vinhnx/VTCode/discussions). If this file and the linked guides disagree, the guides
describe current behavior; please file an issue so we can fix the statement.
