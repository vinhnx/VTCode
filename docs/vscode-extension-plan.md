# VTCode Companion Extension Checklist

This checklist tracks the integration tasks derived from the requested VS Code documentation.

- [x] **VS Code API Reference** – Centralized CLI availability checks behind `refreshCliAvailability`, refreshed status bar state with `vscode.commands.executeCommand` context updates, and funneled command execution through shared helpers for host-aware behavior.
- [x] **Using Proposed API** – Added `ensureStableApi` to log when any proposed APIs are enabled so the extension avoids silently depending on unstable surfaces.
- [x] **TSLint → ESLint Migration** – Replaced the placeholder lint script with an ESLint toolchain (`.eslintrc.cjs`, `.eslintignore`, npm `lint` script) targeting the TypeScript sources.
- [x] **Python Extension Template Guidance** – Mirrored the template’s environment gating by detecting the VTCode CLI, toggling a persisted `vtcode.cliAvailable` context, and wiring configuration change listeners for responsive feature enablement.
- [x] **Contribution Points** – Expanded the manifest with command palette visibility rules, quick action view commands, configuration metadata, and custom theme color contributions.
- [x] **Activation Events** – Declared activation events for every contributed command (`vtcode.refreshQuickActions`, `vtcode.openInstallGuide`) to keep startup intentional and lazy.
- [x] **Extension Manifest** – Added discovery metadata (keywords, categories) and ensured new contributions (colors, commands, menus) are represented in `package.json`.
- [x] **Commands Reference** – Applied command categories/icons and introduced the `vtcode.openInstallGuide` workflow surfaced through menus and quick actions.
- [x] **When-Clause Contexts** – Introduced the `vtcode.cliAvailable` context to gate command palette items and editor menus, aligning availability with CLI detection.
- [x] **Theme Color Reference** – Registered `vtcode.statusBarBackground`/`vtcode.statusBarForeground` colors and bound the status bar item to the new theme colors.
- [x] **Icons in Labels** – Added codicon-backed icons to command contributions and quick action listings for better visual scanning.
- [x] **Document Selector Reference** – Exported a shared `VT_CODE_DOCUMENT_SELECTOR` that enumerates `file`/`untitled` schemes alongside the TOML pattern for language features.
- [x] **Agent Terminal Integration** – Added a dedicated command and quick action to launch `vtcode chat` inside an integrated terminal when the CLI is available, mirroring the native agent experience.
- [x] **Workspace Analysis Workflow** – Surfaced a `vtcode analyze` runner through quick actions, the command palette, and the VTCode output channel for contextual diagnostics.
