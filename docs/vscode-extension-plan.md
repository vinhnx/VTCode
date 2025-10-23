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

## Development, Release, and Distribution Plan

### Phase 0 – Foundation & Research

1. **Extension Capability Audit** – Inventory current `vscode-extension` features against VTCode product goals, noting gaps in language tooling, terminal integration, and guided onboarding.
2. **Documentation Alignment** – Review VS Code API, extension guides, and VTCode CLI docs to confirm supported hosts (desktop, web) and required permissions.
3. **Developer Environment Setup** – Ensure Node.js, npm, VS Code, `vsce`, and VTCode CLI are installed; configure `.env` secrets for marketplace publishing.
4. **Workflow Definition** – Finalize coding conventions (ESLint/Prettier), testing standards (unit, integration), and bundling requirements (esbuild) in team documentation.

### Phase 1 – Iterative Development

1. **Backlog Grooming** – Break down requested capabilities into GitHub issues referencing the checklist items above.
2. **Feature Implementation Loop**
   - Branch per feature, update `package.json` contributions, and extend `src/extension.ts` modules.
   - Add or refine language services in `src/languageFeatures.ts` using the shared document selector.
   - Introduce telemetry or logging guards in preparation for marketplace release.
3. **Documentation Updates** – Maintain `docs/vscode-extension-plan.md` and in-repo guides alongside each feature branch.

### Phase 2 – Quality Assurance

1. **Static Analysis** – Run `npm run lint`, `npm run compile`, `cargo fmt`, and `cargo clippy` (for shared Rust workspace code) on every PR.
2. **Automated Testing** – Execute `npm test` with the VS Code extension host, adding integration coverage for quick actions, terminals, and language features.
3. **Manual Verification** – Smoke test activation events, command palette entries, tree views, and walkthroughs in VS Code Stable and Insiders across macOS, Windows, and Linux.
4. **Accessibility & UX Review** – Validate status bar contrast using contributed theme colors, confirm keyboard navigation, and align walkthrough content with UX guidelines.

### Phase 3 – Release Readiness

1. **Version Management** – Bump `package.json` version, update `CHANGELOG.md`, and tag Git commits with semantic version numbers.
2. **Bundling & Artifacts** – Build the optimized extension bundle via `npm run package` (esbuild) and validate contents against `.vscodeignore`.
3. **Marketplace Validation** – Run `vsce ls`/`vsce package` locally, then `vsce publish --dry-run` to ensure metadata and assets meet marketplace rules.
4. **Security Review** – Audit third-party dependencies with `npm audit` and `cargo deny`, documenting any exceptions.

### Phase 4 – Distribution & Adoption

1. **Publishing** – Publish the VSIX to the Visual Studio Code Marketplace using service principal credentials; mirror the release on GitHub Releases with the bundled artifact.
2. **Distribution Channels** – Provide installation instructions in the VTCode README, website, and welcome walkthrough; offer VSIX downloads for offline users.
3. **Telemetry & Feedback** – Monitor marketplace analytics, collect user feedback via GitHub issues, and triage incoming bug reports.
4. **Support & Iteration** – Schedule monthly review meetings to prioritize updates, address compatibility changes from new VS Code releases, and refresh documentation.
