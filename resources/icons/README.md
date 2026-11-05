# VT Code app and terminal profile icons

Current artwork is the `vtcode-app-*` set: Light is a sand tile with a charcoal `V`, Dark is a
charcoal tile with a warm-white `V`, Dynamic is a diagonal split tile, Clear is the glass `V` on
transparency (no tile), and Tinted is a monochrome slate tile with a white `V`. Use the smallest icon
that looks sharp; larger files are for app bundles and in-app display only.

| File | Size | Use |
| --- | --- | --- |
| `vtcode-app-light-128.png` | 128x128, 7.5K | iTerm2 HiDPI profile artwork (light mode). Embedded by `vtcode-core/build.rs`. |
| `vtcode-app-dark-128.png` | 128x128, 7.1K | iTerm2 HiDPI profile artwork (dark mode). Embedded by `vtcode-core/build.rs`. |
| `vtcode-app-clear-128.png` | 128x128, 4.8K | iTerm2 HiDPI profile artwork (clear glass V). Embedded by `vtcode-core/build.rs`. |
| `vtcode-app-tinted-128.png` | 128x128, 6.7K | iTerm2 HiDPI profile artwork (tinted slate). Embedded by `vtcode-core/build.rs`. |
| `vtcode-app-dynamic-128.png` | 128x128, 7.5K | Mode-neutral split tile for manual setups (Windows Terminal `settings.json` `"icon"`). |
| `vtcode-app-light-512.png` | 512x512, 36K | Desktop Dock artwork (light mode) and Appearance picker preview. |
| `vtcode-app-dark-512.png` | 512x512, 34K | Desktop Dock artwork (dark mode) and Appearance picker preview. |
| `vtcode-app-clear-512.png` | 512x512, 38K | Desktop Dock artwork (clear glass V) and Appearance picker preview. |
| `vtcode-app-tinted-512.png` | 512x512, 36K | Desktop Dock artwork (tinted slate) and Appearance picker preview. |
| `vtcode-app-dynamic-512.png` | 512x512, 40K | Desktop in-app logo, `Dynamic` picker preview, and bundle-icon source art. |
| `vtcode-app-dynamic.icns` | 285K | Prebuilt macOS bundle icon fallback, installed as `VTCode.icns` (used on macOS < 26). |
| `AppIcon.icon/` | 35K | Layered Liquid Glass source: `icon.json` + `Assets/Foreground.png` (1024 V glyph mask). |
| `liquid-glass/` | 380K | Tahoe appearance sources: per-variant layer pairs (see its `README.md`). |
| `vtcode.svg` | vector, 245B | Scalable source for docs. |
| `vtcode-mark.svg` | vector, 195B | Outline `V` mark; the desktop welcome view draws it in the active theme. |
| `vtcode-mono.svg` | vector, 195B | Monochrome V mark (white on transparent) for program registries. |
| `vtcode-mono-32.png` | 32x32, 308B | Mono raster handoff for small icon tiles. |
| `vtcode-mono-120.png` | 120x120, 827B | Desktop menu-bar status icon (AppKit template) + HiDPI tile handoff. |

The legacy `vtcode-profile-*.png` set (yellow `V` mark from `vtchat_resources/v2_1`) was removed; the
companion plugin now vendors `vtcode-app-dynamic-512.png` as its logo/composer icon, and new manual terminal
setups should use the `vtcode-app-*` set.

## Variant selection

`Dynamic` (default) follows the active light/dark mode; `Light`/`Dark`/`Clear`/`Tinted` pin one artwork. Per the
artwork notes, automatic switching selects Light or Dark art at runtime — the Dynamic split tile itself is only used
where the context cannot switch (bundle icon, manual setups, picker preview).

- Desktop: Settings → Appearance → App Icon picker (Dynamic/Light/Dark/Clear/Tinted). The running app applies the
  selection to the Dock via `NSApplication.setApplicationIconImage` at startup, on selection, and on theme-mode
  changes while Dynamic is selected. Finder always shows the bundle (Dynamic) icon.
- CLI: `ui.app_icon = "dynamic"|"light"|"dark"|"clear"|"tinted"` in `vtcode.toml` (default `"dynamic"`). The iTerm2
  tab icon installer resolves Dynamic against the active CLI theme at startup and reinstalls on drift, so a
  theme-mode change applies on the next launch. Switch immediately with
  `/terminal-setup icon light|dark|clear|tinted|dynamic`.

## Liquid Glass bundle icon (macOS 26+)

`AppIcon.icon/` is a hand-authored Icon Composer bundle: sand/Charcoal `automatic-gradient` background
fills (light/dark) plus one glass `V` glyph group with charcoal/cream fills and `tinted → automatic`
specializations, so Tahoe composes Default/Dark/Clear/Tinted at display time. The glyph mask was
extracted from the 1024px masters (geometry verified identical across variants). Only this layered
path is system-shaped: macOS clips neither `.icns` art nor `setApplicationIconImage` images, so the
flat `vtcode-app-*` PNGs and the `.icns` fallback carry the rounded squircle with transparency baked
in (full-bleed opaque squares render as squares in the Dock and Finder).

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

## Regenerating the flat art and fallback icns

After an artwork update, bake the squircle mask into the `vtcode-app-*.png` files and stage the
Dynamic iconset with `scripts/make-macos-app-icons.py` (requires Pillow), then build the fallback:

```bash
python3 scripts/make-macos-app-icons.py \
  --light /path/to/flat-light-1024.png \
  --dark /path/to/flat-dark-1024.png \
  --dynamic /path/to/flat-dynamic-1024.png \
  --clear resources/icons/liquid-glass/Clear \
  --tinted resources/icons/liquid-glass/Tinted
iconutil -c icns /tmp/VTCode.iconset -o resources/icons/vtcode-app-dynamic.icns
iconutil -c iconset resources/icons/vtcode-app-dynamic.icns -o /tmp/check.iconset
```

Each variant flag is optional (omitted variants keep their shipped files); pass a square PNG master
or a layer directory holding `01-background.png` + `02-V-foreground.png`, which the script composites
back-to-front. PNG inputs may be flat or already masked; the script upscales to a 1024 working size,
scales the art into the centered 824px content box, applies an antialiased superellipse mask, and
downscales to the shipped sizes (premultiplied resampling, so tile edges keep their color under
partial alpha). Clear carries no tile, so its gate instead requires transparent margins with the opaque
fraction in the 0.005-0.15 glass-V band. The desktop `embedded_artwork_carries_macos_rounded_shape`
test rejects full-bleed squares. `iconutil`'s icns-write
path is broken on some hosts (older macOS 27 builds reject even previously working iconsets while the
read path works); generate on a host where it succeeds (verified on macOS 27.0.1).

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
- iTerm2 (manual): Settings > Profiles > General > Icon, choose `vtcode-app-light-128.png`,
  `vtcode-app-dark-128.png`, `vtcode-app-clear-128.png`, or `vtcode-app-tinted-128.png` to match the
  terminal theme (the automatic installer picks for you).
- Windows Terminal: set the profile `"icon"` to `resources/icons/vtcode-app-dynamic-128.png`.
- VS Code / Zed / Warp: these use theme/picker icons, not image files; keep the runtime `OSC 1`/`OSC 2` label and
  optionally reference `vtcode.svg` in docs.
- Kitty / Ghostty / WezTerm / Alacritty / Terminal.app / Hyper / Tabby: window icons come from the OS app bundle; no
  per-profile image needed. The runtime icon label still applies to tabs and taskbars.
