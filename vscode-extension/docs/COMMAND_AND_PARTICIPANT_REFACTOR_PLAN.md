# VT Code VSCode Extension - Command & Participant Refactor Plan

This document proposes concrete, incremental refactors for the VT Code VSCode extension to:

-   Modularize commands currently implemented inline in [`src/extension.ts`](src/extension.ts:154).
-   Introduce a participant-style context system aligned with the roadmap and existing `IdeContextFileBridge`.
-   Preserve behavior, command IDs, safety guarantees, and existing UX.

It is implementation-ready and maps directly onto the current codebase.

---

## 1. Goals

1. Reduce `src/extension.ts` complexity by extracting self-contained modules.
2. Centralize command wiring via a `CommandRegistry`.
3. Introduce a `ParticipantRegistry` abstraction to manage context providers:
    - Reuse existing IDE-context logic instead of rewriting.
    - Prepare for future `@workspace`, `@code`, `@git`, `@terminal` participants.
4. Maintain:
    - Workspace trust checks.
    - CLI availability checks.
    - Human-in-the-loop behavior.
    - Existing command IDs & contributions.

All changes are intentionally additive and compatible with the existing manifest.

---

## 2. Command System Refactor

### 2.1 New Types: `src/types/command.ts`

Define a shared contract used by all commands.

```ts
// src/types/command.ts
import * as vscode from "vscode";
import { VtcodeBackend } from "../vtcodeBackend";
import { VtcodeConfigSummary } from "../vtcodeConfig";

export interface CommandServices {
    readonly context: vscode.ExtensionContext;
    readonly backend: VtcodeBackend;
    readonly output: vscode.OutputChannel;
    readonly getConfigSummary: () => VtcodeConfigSummary | undefined;
    readonly ensureWorkspaceTrustedForCommand: (
        action: string
    ) => Promise<boolean>;
    readonly ensureCliAvailableForCommand: () => Promise<boolean>;
}

export interface ICommand {
    readonly id: string;
    execute(...args: unknown[]): Promise<void>;
}
```

Notes:

-   `CommandServices` is injected into each command to avoid re-reading globals.
-   `ICommand` stays minimal; per-command logic uses helpers via closure or constructor.

### 2.2 Registry: `src/commandRegistry.ts`

Centralizes command registration and shared error handling.

```ts
// src/commandRegistry.ts
import * as vscode from "vscode";
import { ICommand, CommandServices } from "./types/command";

export class CommandRegistry {
    private readonly commands = new Map<string, vscode.Disposable>();

    constructor(
        private readonly services: CommandServices,
        private readonly register: typeof vscode.commands.registerCommand = vscode
            .commands.registerCommand
    ) {}

    /**
     * Register a VT Code command and track its disposable.
     */
    registerCommand(command: ICommand): void {
        if (this.commands.has(command.id)) {
            throw new Error(`Command already registered: ${command.id}`);
        }

        const disposable = this.register(
            command.id,
            async (...args: unknown[]) => {
                try {
                    await command.execute(...args);
                } catch (error) {
                    const message =
                        error instanceof Error ? error.message : String(error);
                    this.services.output.appendLine(
                        `[error] Command "${command.id}" failed: ${message}`
                    );
                    void vscode.window.showErrorMessage(
                        `VT Code: "${command.id}" failed: ${message}`
                    );
                }
            }
        );

        this.commands.set(command.id, disposable);
        this.services.context.subscriptions.push(disposable);
    }

    dispose(): void {
        for (const disposable of this.commands.values()) {
            disposable.dispose();
        }
        this.commands.clear();
    }
}
```

Behavior:

-   All commands share:
    -   Same output channel.
    -   Same error-handling pattern.
    -   Same trust/CLI helpers (injected via `CommandServices`).

### 2.3 Example Commands

Below are representative extractions from existing inline logic in [`src/extension.ts`](src/extension.ts:389).

#### 2.3.1 Ask Agent Command

```ts
// src/commands/askAgentCommand.ts
import * as vscode from "vscode";
import { ICommand, CommandServices } from "../types/command";
import { appendIdeContextToPrompt } from "../ideContext"; // see 2.4

export class AskAgentCommand implements ICommand {
    readonly id = "vtcode.askAgent";

    constructor(private readonly services: CommandServices) {}

    async execute(): Promise<void> {
        const { ensureCliAvailableForCommand, output } = this.services;

        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const question = await vscode.window.showInputBox({
            prompt: "What would you like the VT Code agent to help with?",
            placeHolder: "Summarize src/main.rs",
            ignoreFocusOut: true,
        });

        if (!question || !question.trim()) {
            return;
        }

        try {
            const promptWithContext = await appendIdeContextToPrompt(question, {
                includeActiveEditor: true,
            });

            // Preserve behavior: delegate to existing runVtcodeCommand
            // (imported from a shared helper rather than re-implemented here)
            await vscode.commands.executeCommand(
                "vtcode.internal.runCliAsk",
                promptWithContext
            );

            void vscode.window.showInformationMessage(
                "VT Code finished processing your request. Check the VT Code output channel for details."
            );
        } catch (error) {
            const message =
                error instanceof Error ? error.message : String(error);
            output.appendLine(`[error] vtcode.askAgent failed: ${message}`);
            void vscode.window.showErrorMessage(
                `Failed to ask VT Code: ${message}`
            );
        }
    }
}
```

Notes:

-   `vtcode.internal.runCliAsk` is a suggested internal helper command that wraps the existing `runVtcodeCommand(["ask", ...])` implementation from `extension.ts`, allowing reuse without code duplication.
-   Alternatively, expose a shared `runVtcodeAsk(prompt)` utility module.

#### 2.3.2 Ask Selection Command

```ts
// src/commands/askSelectionCommand.ts
import * as vscode from "vscode";
import { ICommand, CommandServices } from "../types/command";

export class AskSelectionCommand implements ICommand {
    readonly id = "vtcode.askSelection";

    constructor(private readonly services: CommandServices) {}

    async execute(): Promise<void> {
        const { ensureCliAvailableForCommand } = this.services;

        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            void vscode.window.showWarningMessage(
                "Open a text editor to ask VT Code about the current selection."
            );
            return;
        }

        const selection = editor.selection;
        if (selection.isEmpty) {
            void vscode.window.showWarningMessage(
                "Highlight text first, then run “Ask About Selection with VT Code.”"
            );
            return;
        }

        const selectedText = editor.document.getText(selection);
        if (!selectedText.trim()) {
            void vscode.window.showWarningMessage(
                "The selected text is empty. Select code or text for VT Code to inspect."
            );
            return;
        }

        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const defaultQuestion = "Explain the highlighted selection.";
        const question = await vscode.window.showInputBox({
            prompt: "How should VT Code help with the highlighted selection?",
            value: defaultQuestion,
            valueSelection: [0, defaultQuestion.length],
            ignoreFocusOut: true,
        });

        if (question === undefined) {
            return;
        }

        const trimmedQuestion = question.trim() || defaultQuestion;
        const languageId = editor.document.languageId || "text";
        const rangeLabel = `${selection.start.line + 1}-${
            selection.end.line + 1
        }`;
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(
            editor.document.uri
        );
        const relativePath = workspaceFolder
            ? vscode.workspace.asRelativePath(editor.document.uri, false)
            : editor.document.fileName;
        const normalizedSelection = selectedText.replace(/\r\n/g, "\n");

        const prompt = `${trimmedQuestion}

File: ${relativePath}
Lines: ${rangeLabel}

\`\`\`${languageId}
${normalizedSelection}
\`\`\``;

        await vscode.commands.executeCommand(
            "vtcode.internal.runCliAsk",
            prompt
        );
    }
}
```

#### 2.3.3 Other Commands

Similarly extract:

-   `vtcode.openConfig` → `OpenConfigCommand`
-   `vtcode.openDocumentation` → `OpenDocsCommand`
-   `vtcode.launchAgentTerminal` → `LaunchAgentTerminalCommand`
-   `vtcode.runAnalyze` → `RunAnalyzeCommand`
-   Trust / HITL / MCP / tools policy commands.

Each:

-   Uses `CommandServices` for trust/CLI/summary.
-   For non-CLI commands (e.g. open docs), skip CLI checks but keep trust if needed.

### 2.4 Supporting Extraction: IDE Context Helpers

Create `src/ideContext.ts` as a thin wrapper around existing helpers currently in [`src/extension.ts`](src/extension.ts:2712):

-   Export:
    -   `appendIdeContextToPrompt`
    -   `buildIdeContextBlock`
    -   Any pure helpers needed by commands or participants.

This allows:

-   Commands and participants to call `appendIdeContextToPrompt(...)` without importing private internals from `extension.ts`.
-   Incremental migration without changing behavior.

---

## 3. Participant / Context System

The current extension already:

-   Builds a VS Code Chat participant: `vtcode.agent` in `registerVtcodeAiIntegrations`.
-   Uses `appendIdeContextToPrompt` and `IdeContextFileBridge` to enrich prompts.

We formalize this into a small participant system that:

-   Stays internal (no breaking API).
-   Makes it easy to add more context sources later.

### 3.1 Types: `src/types/participant.ts`

```ts
// src/types/participant.ts
import type * as vscode from "vscode";

export interface ParticipantContext {
    readonly chatRequest?: vscode.ChatRequest;
    readonly cancellationToken?: vscode.CancellationToken;
}

export interface ChatParticipant {
    readonly id: string;
    readonly displayName: string;
    readonly priority: number;

    canHandle(context: ParticipantContext): boolean;

    /**
     * Given the current message and context, return an augmented prompt.
     * Implementations should be defensive, fast, and respect cancellation.
     */
    resolve(message: string, context: ParticipantContext): Promise<string>;
}
```

### 3.2 Registry: `src/participants/participantRegistry.ts`

```ts
// src/participants/participantRegistry.ts
import { ChatParticipant, ParticipantContext } from "../types/participant";

export class ParticipantRegistry {
    private readonly participants: ChatParticipant[] = [];

    register(participant: ChatParticipant): void {
        this.participants.push(participant);
        this.participants.sort((a, b) => a.priority - b.priority);
    }

    async resolveMessage(
        message: string,
        context: ParticipantContext
    ): Promise<string> {
        let current = message;
        for (const participant of this.participants) {
            if (!participant.canHandle(context)) {
                continue;
            }
            try {
                current = await participant.resolve(current, context);
            } catch (error) {
                // Best-effort; log in caller
            }
        }
        return current;
    }
}
```

### 3.3 Initial Participants

Use existing behavior instead of inventing new features:

1. `WorkspaceContextParticipant`

    - Uses `appendIdeContextToPrompt` with `includeActiveEditor` / `includeVisibleEditors`.
    - Essentially wraps current IDE context behavior into a participant.

2. Later (Phase 3+):
    - `GitParticipant`, `TerminalParticipant`, etc. based on roadmap.

Example:

```ts
// src/participants/workspaceContextParticipant.ts
import { ChatParticipant, ParticipantContext } from "../types/participant";
import { appendIdeContextToPrompt } from "../ideContext";

export class WorkspaceContextParticipant implements ChatParticipant {
    readonly id = "vtcode.workspaceContext";
    readonly displayName = "Workspace Context";
    readonly priority = 10;

    canHandle(_context: ParticipantContext): boolean {
        return true;
    }

    async resolve(
        message: string,
        context: ParticipantContext
    ): Promise<string> {
        return appendIdeContextToPrompt(message, {
            includeActiveEditor: true,
            chatRequest: context.chatRequest,
            cancellationToken: context.cancellationToken,
        });
    }
}
```

### 3.4 Wiring with VS Code Chat Participant

In `registerVtcodeAiIntegrations` (in [`src/extension.ts`](src/extension.ts:2545)):

-   Instantiate `ParticipantRegistry`.
-   Register `WorkspaceContextParticipant`.
-   When handling `vtcode.agent` chat:

    1. Start from `request.prompt`.
    2. Build `ParticipantContext` with `request` and `token`.
    3. Call `participantRegistry.resolveMessage(basePrompt, context)`.
    4. Pass the resolved prompt to the existing `runVtcodeCommand(["ask", ...])`.

This:

-   Keeps current behavior identical.
-   Makes it trivial to add more participants following the patterns in `VSCODE_EXTENSION_CODE_EXAMPLES.md`.

---

## 4. Migration Strategy

Implement incrementally without breaking users:

1. Add:
    - `src/types/command.ts`
    - `src/commandRegistry.ts`
    - `src/types/participant.ts`
    - `src/participants/participantRegistry.ts`
    - `src/participants/workspaceContextParticipant.ts`
    - `src/ideContext.ts` (extracted helpers)
2. Wire `CommandRegistry` in `activate`:
    - Construct `CommandServices` from existing globals/helpers.
    - Register new command classes.
    - Keep old inline registrations temporarily for parity testing.
3. Once verified:
    - Remove duplicate inline command implementations from `extension.ts`.
4. Wire `ParticipantRegistry` into `registerVtcodeAiIntegrations`:
    - Use registry to produce final prompts for `vtcode.agent`.
    - Keep `appendIdeContextToPrompt` behavior identical.
5. Add targeted tests:
    - Unit tests for:
        - `CommandRegistry`
        - `AskAgentCommand`, `AskSelectionCommand`
        - `WorkspaceContextParticipant`
    - Integration-style test for:
        - `vtcode.agent` chat path with participant resolution.

---

## 5. Tool Approval UI & Conversation Persistence (High-Level Hooks)

Detailed UX and code are covered in:

-   `VSCODE_EXTENSION_CODE_EXAMPLES.md`

This refactor plan prepares for them by:

-   Centralizing commands (so tool-approval-triggering actions are easy to wrap).
-   Introducing participants (so context for approval can be richer).
-   Keeping `ChatViewProvider` as the single chat UI integration point.

Recommended future hooks (not implemented yet):

-   `src/types/toolApproval.ts` + `src/tools/toolApprovalManager.ts`:
    -   Used by `ChatViewProvider` and `VtcodeBackend` to:
        -   Show approval modals in webview.
        -   Enforce HITL + policies from `vtcode.toml`.
-   `src/chat/conversationManager.ts`:
    -   Provide thread-level persistence.
    -   Integrated into:
        -   Webview (conversation list).
        -   VS Code Chat (optional mapping).

These can be implemented after this refactor without further structural upheaval.

---

## 6. Summary

This plan:

-   Aligns the current extension with the documented roadmap.
-   Provides concrete file-level changes:
    -   Command interfaces + registry.
    -   Participant interfaces + registry.
    -   IDE context helper extraction.
-   Keeps all existing behaviors and safeguards.
-   Sets a clean foundation for:
    -   Tool approval UI
    -   Conversation persistence
    -   Additional participants and UI polish.

Use this as the canonical reference when starting the Phase 2 refactor in this repository.
