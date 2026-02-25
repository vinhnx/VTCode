# VT Code VSCode Extension - Developer Quick Start

This guide is for contributors implementing the VT Code VSCode extension roadmap (Phases 1–2). It explains how to run, debug, and extend the extension using the existing architecture and the new docs in this repo.

---

## 1. Prerequisites

-   Node.js 18+
-   npm (or pnpm/yarn)
-   VS Code (latest stable)
-   VT Code CLI installed and on PATH:
    -   `cargo install vtcode`
    -   or `brew install vtcode`
    -   or `npm install -g vtcode-ai`

Recommended:

-   Rust toolchain (for working on the core VT Code repo)
-   Git + a feature branch per change

Workspace trust is required to run CLI-dependent features inside VS Code.

---

## 2. Project Layout (VSCode Extension)

Key paths:

-   `vscode-extension/`
    -   Extension root (this package).
-   `src/extension.ts`
    -   Main entrypoint:
        -   Activation, wiring, context keys.
        -   Registers commands, tasks, views.
        -   Integrates with VT Code CLI, MCP, and VS Code AI APIs.
-   `src/chatView.ts`
    -   Webview chat UI provider (VT Code Chat panel).
-   `src/vtcodeBackend.ts`
    -   CLI integration wrapper (spawns `vtcode`, streams output).
-   `src/vtcodeConfig.ts`
    -   `vtcode.toml` summary & helpers (MCP, tool policy, HITL).
-   `src/languageFeatures.ts`
    -   Language features for VT Code-related files.
-   `vscode-extension/docs/ARCHITECTURE.md`
    -   High-level architecture (read this next).
-   `docs/vscode-extension-improve-docs/*`
    -   Full improvement analysis, examples, migration roadmap.

Planned (Phase 2+):

-   `src/types/command.ts` / `src/commands/*`
-   `src/types/participant.ts` / `src/participants/*`
-   `src/chat/*` for state & conversations
-   `src/tools/*` for tool approval and risk handling
-   `src/ui/*` for UI components & status indicators

---

## 3. Running the Extension

From `vscode-extension/`:

```bash
npm install
npm run compile
npm run watch        # optional: incremental build
```

Then in VS Code:

1. Open the `vscode-extension/` folder in VS Code.
2. Run:
    - `Run and Debug` → `Launch Extension`
3. In the Extension Development Host:
    - Trust the workspace when prompted.
    - Ensure `vtcode` is on PATH (see status bar or VT Code views).

Verification checklist:

-   `VT Code` status bar item appears.
-   `VT Code Quick Actions` and `VT Code Workspace Status` views show in the Explorer / View container.
-   `VT Code: Ask the Agent` command is available.
-   `VT Code: Open Chat` opens the VT Code chat view.

If CLI is missing/untrusted, extension will:

-   Show actionable warnings.
-   Keep risky commands disabled.

---

## 4. Core Concepts (What You Must Know)

Read these before changing code:

1. Architecture:
    - [`ARCHITECTURE.md`](ARCHITECTURE.md)
    - [`src/extension.ts`](src/extension.ts)
2. CLI Bridge:
    - [`src/vtcodeBackend.ts`](src/vtcodeBackend.ts)
    - Always call through this instead of spawning processes manually.
3. Safety:
    - Workspace trust gating (`ensureWorkspaceTrustedForCommand`).
    - CLI availability (`ensureCliAvailableForCommand` + `refreshCliAvailability`).
    - HITL and `automation.full_auto` behavior.
4. IDE Context:
    - `IdeContextFileBridge`
    - `appendIdeContextToPrompt(...)` and related helpers.

If you’re unsure where to plug something in:

-   Keep all orchestration in `extension.ts`.
-   Keep low-level integration in dedicated modules.
-   Reuse the patterns in `VSCODE_EXTENSION_CODE_EXAMPLES.md`.

---

## 5. Implementing Phase 1–2 (Practical Steps)

This section gives a concrete, minimal sequence aligned with the docs.

### Step 1: UI / Status / Error Quick Wins (Phase 1)

Goal: Improve UX without changing semantics.

-   Enhance chat webview:
    -   Use existing HTML/CSS to polish markdown, code blocks, copy buttons.
-   Integrate a lightweight status indicator:
    -   Surface:
        -   idle / thinking / streaming / executing / error
        -   tokens (if available)
        -   elapsed time (where reasonable)
-   Use a central error presentation helper:
    -   Wrap raw errors with clear messages in chat + output channel.

Implementation notes:

-   Keep existing commands and entrypoints intact.
-   Only wire new UI components through `ChatViewProvider` and webview messaging.
-   No breaking changes to command IDs or settings.

### Step 2: Command Modularization (Phase 2)

Goal: Extract command logic out of `extension.ts` while preserving behavior.

Suggested pattern (see code examples docs):

1. Create `src/types/command.ts`:
    - Define `CommandContext` and `ICommand` interface.
2. Create `src/commandRegistry.ts`:
    - Responsible for:
        - Constructing a shared `CommandContext`.
        - Registering VS Code commands for each `ICommand`.
        - Centralizing error handling and trust/CLI checks.
3. For each existing inline command in `extension.ts`:
    - Create `src/commands/{name}Command.ts`.
    - Move logic into an `ICommand` implementation.
    - Use dependency injection for:
        - `VtcodeBackend`
        - Output channel
        - Any config helpers
4. In `activate()`:
    - Instantiate `CommandRegistry`.
    - Register all commands via the registry.
    - Remove duplicate inline implementations once parity is verified.

Constraints:

-   Do not change contributed command IDs (e.g. `vtcode.askAgent`, `vtcode.askSelection`).
-   Reuse existing trust/CLI helpers where possible.
-   Add tests around command classes before deleting inline versions.

### Step 3: Participant / Context System (Phase 2)

Goal: Make context providers composable and testable.

-   Introduce `src/types/participant.ts`.
-   Implement:
    -   `ParticipantRegistry` (manages participants, ordering).
    -   `WorkspaceParticipant`, `CodeParticipant`, etc.
-   Gradually migrate:
    -   IDE context / reference augmentation logic from `extension.ts` into participants.
-   Connect:
    -   VS Code Chat participant (`vtcode.agent`) to `ParticipantRegistry`.
    -   In the future, `ChatViewProvider` may call into the same registry.

Keep:

-   Same safety & truncation limits as current `IdeContextFileBridge`.
-   Backwards-compatible behavior while refactoring.

---

## 6. Testing and Validation

Before opening a PR:

-   Build:
    -   `npm run compile`
-   Lint (if configured in this package):
    -   `npm run lint`
-   Run tests (once test harness is added):
    -   `npm test`
-   Manual checks:
    -   Verify commands:
        -   `VT Code: Ask the Agent`
        -   `VT Code: Ask About Selection`
        -   `VT Code: Launch Agent Terminal`
        -   `VT Code: Analyze Workspace`
    -   Verify:
        -   Workspace trust prompts behave as documented.
        -   CLI missing / misconfigured flows are clear.
        -   MCP configuration commands operate as expected.

Keep log output consistent:

-   `[info] ...`
-   `[warn] ...`
-   `[error] ...`

---

## 7. Implementation Rules

When contributing changes:

-   Do:
    -   Keep extension behavior backward compatible.
    -   Use small, composable modules (commands, participants, tools).
    -   Inject `VtcodeBackend` and shared services; do not new them ad-hoc in many places.
    -   Respect workspace trust and HITL in all new flows.
    -   Align with patterns from:
        -   `VSCODE_EXTENSION_CODE_EXAMPLES.md`
-   Do not:
    -   Bypass `VtcodeBackend` with raw `spawn` calls.
    -   Introduce breaking command ID or setting changes without a migration plan.
    -   Depend on unstable VS Code proposals as hard requirements.
    -   Implement “full-auto” execution inside VS Code.

---

## 8. Where to Go Next

For deeper details:

-   Architecture:
    -   [`ARCHITECTURE.md`](ARCHITECTURE.md)
-   Improvements & examples:
    -   `docs/vscode-extension-improve-docs/VSCODE_EXTENSION_CODE_EXAMPLES.md`
    -   `docs/vscode-extension-improve-docs/VSCODE_EXTENSION_MIGRATION_ROADMAP.md`
-   Phase 2 guidance:
    -   `vscode-extension/docs/PHASE_2_DEVELOPER_GUIDE.md`
    -   `vscode-extension/IMPROVEMENT_ROADMAP_INDEX.md`

Use this Quick Start as your checklist to get productive on the VT Code VSCode extension within minutes.
