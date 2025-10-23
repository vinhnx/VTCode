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
- [x] **Human-in-the-Loop & Tool Policies** – Surface HITL status in the status bar, enable quick toggles, and provide direct access to `vtcode.toml` tool policy sections and documentation.
- [x] **MCP Configuration Flow** – Watch vtcode configuration files, expose MCP provider management from VS Code, and guide users to the MCP integration documentation.
- [x] **Workspace-Aware CLI Invocation** – Forward the active `vtcode.toml` path when launching VTCode commands or the integrated chat terminal so sessions respect workspace configuration and tool policies.

## Development, Release, and Distribution Plan

### Hands-on Development Guide

1. **Install prerequisites**
   1. Install [Node.js 18+](https://nodejs.org/) and confirm `npm --version` works in your shell.
   2. Install the [Visual Studio Code](https://code.visualstudio.com/) desktop application and the official **VS Code Extension Development** workload.
   3. Globally install the VS Code Extension CLI with `npm install -g @vscode/vsce` (required for packaging and publishing).
   4. Ensure the VTCode CLI is available on your `PATH`; the extension surfaces richer quick actions when it detects the binary.
2. **Bootstrap the workspace**
   1. From the repository root run `npm install` in `vscode-extension/` to restore Node dependencies.
   2. Run `npm run compile` once to produce the initial `out/` directory consumed by the debugger.
   3. Optionally execute `npm run lint` to confirm ESLint passes before opening VS Code.
3. **Open and configure VS Code**
   1. Launch VS Code in the repo root with `code .`.
   2. When prompted, trust the workspace and install any recommended extensions (ESLint, VS Code Extension Test Runner).
   3. Open the **Run and Debug** view; the project already includes two launch configurations: **Run Extension** and **Launch Integration Tests**.
4. **Run the extension in the Extension Development Host**
   1. Select **Run Extension** and press `F5` (or click **Run**). VS Code builds the project and opens a secondary Extension Development Host window.
   2. In the new window, open or create a VTCode workspace. The VTCode Companion activates automatically once a `vtcode.toml` file is present or a contributed command is invoked.
   3. Test commands from the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`), the VTCode Quick Actions view, and the status bar entry.
   4. Use the **Developer: Reload Window** command to iterate quickly after code changes. The debugger automatically reattaches on reload.
5. **Debug and inspect runtime behavior**
   1. Set breakpoints inside `src/extension.ts` or `src/languageFeatures.ts`; the primary VS Code window suspends execution when the extension hits those breakpoints in the development host.
   2. Inspect the **VTCode** output channel (created by the extension) to monitor CLI invocations and configuration forwarding.
   3. Use the **Debug Console** to evaluate variables while paused.
6. **Run automated tests**
   1. Back in the primary VS Code window, choose the **Launch Integration Tests** configuration and press `F5`. This spins up a test instance that executes the suites under `src/test/`.
   2. Alternatively, run `npm test` from the command line; ensure required native libraries (such as `libatk-1.0.so.0`) are available on Linux CI runners.
   3. Maintain parity with repository-wide checks by running `npm run lint`, `npm run compile`, `cargo fmt`, and `cargo clippy` before committing changes.

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

### Packaging and Marketplace Distribution Steps

1. **Prepare release metadata**
   1. Update `CHANGELOG.md` with the upcoming version notes and ensure screenshots or walkthrough assets referenced in `package.json` exist.
   2. Bump the `version` field in `vscode-extension/package.json` and commit the change.
2. **Create the production bundle**
   1. Run `npm run clean && npm run package` inside `vscode-extension/`. This triggers esbuild to emit the minimized `dist/` output and assembles the VSIX staging files under `out/`.
   2. Verify the bundle contents with `npx vsce ls` to confirm only expected assets are included (respecting `.vscodeignore`).
3. **Dry-run packaging**
   1. Execute `npx vsce package` to generate a local `.vsix` file. Inspect the archive (it is a ZIP) to confirm metadata and assets are correct.
   2. Install the VSIX locally with `code --install-extension vtcode-companion-<version>.vsix` and perform a manual smoke test in the Extension Development Host.
4. **Authenticate for publishing**
   1. Create or reuse an Azure DevOps publisher account and generate a Personal Access Token with the `Marketplace (Publish)` scope.
   2. Configure VSCE to use the token via `vsce login <publisher>`; the CLI stores credentials securely for subsequent publishes.
5. **Publish to the Marketplace**
   1. Run `npx vsce publish` (optionally with `--pat <token>` in CI environments) to upload the extension to the Visual Studio Code Marketplace.
   2. Tag the Git repository with the released version (for example, `git tag v0.3.0 && git push --tags`) and attach the `.vsix` file to the corresponding GitHub Release.
6. **Post-release validation**
   1. Verify the Marketplace listing renders correctly (icon, gallery images, README, changelog).
   2. Monitor installation metrics and crash telemetry; address critical feedback with a follow-up patch release if necessary.

### Phase 4 – Distribution & Adoption

1. **Publishing** – Publish the VSIX to the Visual Studio Code Marketplace using service principal credentials; mirror the release on GitHub Releases with the bundled artifact.
2. **Distribution Channels** – Provide installation instructions in the VTCode README, website, and welcome walkthrough; offer VSIX downloads for offline users.
3. **Telemetry & Feedback** – Monitor marketplace analytics, collect user feedback via GitHub issues, and triage incoming bug reports.
4. **Support & Iteration** – Schedule monthly review meetings to prioritize updates, address compatibility changes from new VS Code releases, and refresh documentation.
