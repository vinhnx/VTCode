# VT Code app and terminal profile icons

Current artwork is the `vtcode-app-*` set (flat `V` mark): Light is a sand tile with a charcoal `V`, Dark is a
charcoal tile with a warm-white `V`, and Dynamic is a diagonal split tile. Use the smallest icon that looks sharp;
larger files are for app bundles and in-app display only.

| File                          | Size          | Use                                                                                      |
| ----------------------------- | ------------- | ---------------------------------------------------------------------------------------- |
| `vtcode-app-light-128.png`    | 128x128, 1.2K | iTerm2 HiDPI profile artwork (light mode). Embedded by `vtcode-core/build.rs`.           |
| `vtcode-app-dark-128.png`     | 128x128, 1.4K | iTerm2 HiDPI profile artwork (dark mode). Embedded by `vtcode-core/build.rs`.            |
| `vtcode-app-dynamic-128.png`  | 128x128, 1.6K | Mode-neutral split tile for manual setups (Windows Terminal `settings.json` `"icon"`).  |
| `vtcode-app-light-512.png`    | 512x512, 2.3K | Desktop Dock artwork (light mode) and Appearance picker preview.                         |
| `vtcode-app-dark-512.png`     | 512x512, 3.1K | Desktop Dock artwork (dark mode) and Appearance picker preview.                          |
| `vtcode-app-dynamic-512.png`  | 512x512, 4.6K | Desktop in-app logo, `Dynamic` picker preview, and bundle-icon source art.               |
| `vtcode-app-dynamic.icns`     | 171K          | Prebuilt macOS bundle icon fallback, installed as `VTCode.icns` (used on macOS < 26).    |
| `AppIcon.icon/`               | 35K           | Layered Liquid Glass source: `icon.json` + `Assets/Foreground.png` (1024 V glyph mask).  |
| `vtcode.svg`                  | vector, 245B  | Scalable source for docs.                                                                |
| `vtcode-mark.svg`             | vector, 195B  | Outline `V` mark; the desktop welcome view draws it in the active theme.                 |
| `vtcode-mono.svg`             | vector, 195B  | Monochrome V mark (white on transparent) for program registries.                         |
| `vtcode-mono-32.png`          | 32x32, 308B   | Mono raster handoff for small icon tiles.                                                |
| `vtcode-mono-120.png`         | 120x120, 827B | Mono raster handoff for HiDPI icon tiles.                                                |

The legacy `vtcode-profile-*.png` set (yellow `V` mark from `vtchat_resources/v2_1`) was removed; the
companion plugin now vendors `vtcode-app-dynamic-512.png` as its logo/composer icon, and new manual terminal
setups should use the `vtcode-app-*` set.

## Variant selection

`Dynamic` (default) follows the active light/dark mode; `Light`/`Dark` pin one artwork. Per the artwork notes,
automatic switching selects Light or Dark art at runtime — the Dynamic split tile itself is only used where the
context cannot switch (bundle icon, manual setups, picker preview).

- Desktop: Settings → Appearance → App Icon picker (Dynamic/Light/Dark). The running app applies the selection to
  the Dock via `NSApplication.setApplicationIconImage` at startup, on selection, and on theme-mode changes while
  Dynamic is selected. Finder always shows the bundle (Dynamic) icon.
- CLI: `ui.app_icon = "dynamic"|"light"|"dark"` in `vtcode.toml` (default `"dynamic"`). The iTerm2 tab icon
  installer resolves Dynamic against the active CLI theme at startup and reinstalls on drift, so a theme-mode
  change applies on the next launch. Switch immediately with `/terminal-setup icon light|dark|dynamic`.

## Liquid Glass bundle icon (macOS 26+)

`AppIcon.icon/` is a hand-authored Icon Composer bundle: sand/Charcoal `automatic-gradient` background
fills (light/dark) plus one glass `V` glyph group with charcoal/cream fills and `tinted → automatic`
specializations, so Tahoe composes Default/Dark/Clear/Tinted at display time. The glyph mask was
extracted from the 1024px masters (geometry verified identical across variants); the OS applies its
own squircle mask, so all art stays full-bleed opaque with no baked rounding or transparency.

Build and validate (requires Xcode CLT or full Xcode):

```bash
scripts/build-app-icon.sh resources/icons/AppIcon.icon /tmp/Assets.car
xcrun --sdk macosx assetutil --info /tmp/Assets.car  # Aqua + DarkAqua + Tintable appearances
```

`package-desktop-macos.sh` builds this into `Contents/Resources/Assets.car` and sets
`CFBundleIconName=AppIcon`; `CFBundleIconFile=VTCode.icns` remains the pre-Tahoe fallback.
`Assets.car` is a build artifact (~1.6MB, mostly pre-composited appearance renditions) and is not
committed. The flattened `AppIcon.icns` that `actool` also emits carries only small slots, so it is
not used as the fallback.

## Regenerating the fallback icns

`vtcode-app-dynamic.icns` is prebuilt from the 1024px Dynamic source art. To rebuild after an artwork update:

```bash
mkdir VTCode.iconset
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" VTCode-Dynamic-1024.png --out VTCode.iconset/icon_${size}x${size}.png
  sips -z "$((size * 2))" "$((size * 2))" VTCode-Dynamic-1024.png --out VTCode.iconset/icon_${size}x${size}@2x.png
done
iconutil -c icns VTCode.iconset -o vtcode-app-dynamic.icns
```

`iconutil`'s icns-write path is broken on some hosts (macOS 27 rejects even previously working iconsets while
the read path works); generate on a host where it succeeds and verify with
`iconutil -c iconset vtcode-app-dynamic.icns -o /tmp/check.iconset`.

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
- iTerm2 (manual): Settings > Profiles > General > Icon, choose `vtcode-app-light-128.png` or
  `vtcode-app-dark-128.png` to match the terminal theme (the automatic installer picks for you).
- Windows Terminal: set the profile `"icon"` to `resources/icons/vtcode-app-dynamic-128.png`.
- VS Code / Zed / Warp: these use theme/picker icons, not image files; keep the runtime `OSC 1`/`OSC 2` label and
  optionally reference `vtcode.svg` in docs.
- Kitty / Ghostty / WezTerm / Alacritty / Terminal.app / Hyper / Tabby: window icons come from the OS app bundle; no
  per-profile image needed. The runtime icon label still applies to tabs and taskbars.
