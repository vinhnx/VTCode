# Desktop shell and app icon

## Concept workspace layout

The desktop uses a shared top toolbar above a recessed navigation pane, central
conversation, and contextual inspector. Navigation selections use quiet surfaces;
the accent emphasizes primary actions. The gpui-kit textarea and model/attachment
actions share one composer surface. Existing VT Code theme tokens drive all colors.

<!-- markdownlint-disable MD060 -- intentionally unpadded compact table -->
| Action                         | Control                                 |
| ------------------------------ | --------------------------------------- |
| Return to the workspace        | Navigation rail Home button             |
| Collapse or restore navigation | `Cmd+B` or window-actions overflow menu |
| Search commands                | Toolbar search button or `Cmd+K`        |
<!-- markdownlint-enable MD060 -->

Collapsing navigation retains the active session, draft, and pane width for the
current window. The toolbar remains available when navigation is collapsed.

The toolbar and welcome logo embed `resources/icons/vtcode-app-dynamic-512.png`.
macOS packaging installs the prebuilt `resources/icons/vtcode-app-dynamic.icns`
as `Contents/Resources/VTCode.icns` (pre-Tahoe fallback) and compiles
`resources/icons/AppIcon.icon` into `Contents/Resources/Assets.car`, declaring
`CFBundleIconFile` and `CFBundleIconName=AppIcon` in `Info.plist`; no checkout
paths or external image requests are needed at runtime. On macOS 26+ the OS
composes the layered Liquid Glass Default/Dark/Clear/Tinted appearances from
the catalog at display time. The running app defers `Dynamic` to that
system-composed bundle icon and only overrides the Dock via
`NSApplication.setApplicationIconImage` for explicit pins; on pre-26 it
resolves Dynamic against the active theme mode. The flat PNGs and the `.icns` carry the rounded squircle with
transparency baked in, because macOS clips neither `.icns` art nor
`setApplicationIconImage` images to the system shape (only the layered
`Assets.car` path is system-shaped); see `resources/icons/README.md`. The GPUI Kit
asset source includes its default component icons and only the additional
catalog icons referenced by the desktop UI.
