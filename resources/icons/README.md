# VT Code terminal profile icons

Bundled profile/tab icons sourced from `vtchat_resources/v2_1`. Use the smallest icon that looks sharp; larger files are
for app bundles only.

| File                     | Size          | Use                                                                           |
| ------------------------ | ------------- | ----------------------------------------------------------------------------- |
| `vtcode-profile-32.png`  | 32x32, 1.0K   | Tab bar / low-DPI profile icon. Default choice for most terminals.            |
| `vtcode-profile-120.png` | 120x120, 4.6K | HiDPI profile icon (iTerm2 Retina, Terminal.app).                             |
| `vtcode-profile-180.png` | 180x180, 7.5K | Cross-platform PNG fallback, incl. Windows Terminal `settings.json` `"icon"`. |
| `vtcode-profile-512.png` | 512x512, 29K  | High-res / future app bundle only. Not for tabs.                              |
| `vtcode.svg`             | vector, 474B  | Scalable source.                                                              |
| `vtcode-mono.svg`        | vector, 647B  | Monochrome V mark (white on transparent) for program registries.              |
| `vtcode-mono-32.png`     | 32x32, 574B   | Mono raster handoff for small icon tiles.                                     |
| `vtcode-mono-120.png`    | 120x120, 1.8K | Mono raster handoff for HiDPI icon tiles.                                     |

Artwork: yellow `V` mark sourced from `vtchat_resources/v2_1/logo_dark.png` (`logo_light.png` is byte-identical),
downscaled per size. The mono files redraw the same LigatureSymbols OFL `V` path (`vtcode-mark.svg`) as a
white-on-transparent single-color mark for dark terminal chrome; hosts tint or invert it for light surfaces.

The desktop welcome view draws the SVG's outline V in the active theme without the filled tile.
The toolbar, application icon, and transcript retain the original profile artwork.

## Runtime behavior

VT Code sets both the terminal icon label (`OSC 1`) and window title (`OSC 2`) to the same sanitized status text on
every title update. This keeps the built-in profile/tab icon label in sync on emulators that distinguish icon from
title, instead of relying on `OSC 0` aliasing.

## Program registries (Rex/Ghostty)

Rex renders OSC 7501 records (`app=vtcode`, on by default) as tab/pane status chrome with no per-terminal
install; Ghostty implements the same protocol. The provider icon tile itself comes from each terminal's
built-in program registry keyed by that `app` id, so a `vtcode` tile appears only after the terminal maps it.
The `vtcode-mono.*` files above are the registration artwork for that upstream request; see
[Terminal pane icons](../../docs/development/terminal-program-status.md#terminal-pane-icons-rexghostty) for the
full spec. No escape sequence can set registry artwork from the CLI side.

## Manual graphical icon setup

Text `OSC 1`/`OSC 2` works everywhere. For a graphical icon, configure once per terminal:

- iTerm2 (automatic): run `/terminal-setup install-iterm2-icon` inside VT Code — or just launch VT Code under iTerm2,
  which installs the profile on first run and prints a notice. It installs a `VT Code` dynamic profile (custom icon)
  under `~/Library/Application Support/iTerm2/DynamicProfiles/vtcode.json` with artwork in the VT Code data dir. New
  tabs pick it up immediately. The installer also runs on every interactive iTerm2 launch and rewrites the profile file
  whenever it is missing or the bundled profile/artwork changed, so deleting `vtcode.json` is not a persistent opt-out —
  it reappears on the next launch (there is no config key to disable this; `--quiet` suppresses only the install
  notice). VT Code switches the session to the profile once at TUI startup, only when the file exists, and switches back
  to the session's original profile on exit — so the icon is shown only while the TUI runs. If a tab is already stuck
  showing the icon from an older build, run `/terminal-setup reset-iterm2-icon` in it or open a new tab.
- iTerm2 (manual): Settings > Profiles > General > Icon, choose `vtcode-profile-120.png` (Retina) or
  `vtcode-profile-32.png`.
- Windows Terminal: set the profile `"icon"` to `resources/icons/vtcode-profile-180.png`.
- VS Code / Zed / Warp: these use theme/picker icons, not image files; keep the runtime `OSC 1`/`OSC 2` label and
  optionally reference `vtcode.svg` in docs.
- Kitty / Ghostty / WezTerm / Alacritty / Terminal.app / Hyper / Tabby: window icons come from the OS app bundle; no
  per-profile image needed. The runtime icon label still applies to tabs and taskbars.
