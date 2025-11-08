# VTCode VSCode Extension - Tool Approval UI & Conversation Persistence Plan

This document specifies how to evolve the VTCode VSCode extension to support:

- A first-class, centralized Tool Approval UI.
- Durable, thread-based conversation persistence.

It is aligned with:

- `docs/vscode-extension-improve-docs/VSCODE_EXTENSION_IMPROVEMENTS.md`
- `docs/vscode-extension-improve-docs/VSCODE_EXTENSION_MIGRATION_ROADMAP.md`
- `vscode-extension/docs/ARCHITECTURE.md`
- `vscode-extension/docs/COMMAND_AND_PARTICIPANT_REFACTOR_PLAN.md`

The design is intended to be implemented incrementally without breaking existing behavior.

---

## 1. Goals

1. Replace scattered approval prompts with a single, consistent approval workflow.
2. Integrate approval into the VTCode Chat UI (webview) and VS Code LM tools where appropriate.
3. Persist conversations (threads) across sessions:
   - Enable switching between threads.
   - Avoid re-sending entire history when unnecessary.
4. Preserve:
   - Workspace trust and HITL semantics from `vtcode.toml`.
   - Existing command IDs and user workflows.
   - Compatibility with desktop vs. web hosts (no CLI on web).

All APIs and file names are internal to this extension and can evolve as needed.

---

## 2. Tool Approval UI

### 2.1 Requirements

- User must see:
  - Tool name and description.
  - Arguments / paths / commands.
  - Risk classification (low / medium / high).
- User must be able to:
  - Approve and run as-is.
  - Deny.
  - Optionally adjust arguments (for some tools).
- Respect configuration:
  - `human_in_the_loop`:
    - If true (default): approval required for non-trivial tools.
    - If false: optionally auto-approve a configured allowlist.
  - Tool policy / allowlist / denylist (from `vtcode.toml`).
- Work both for:
  - Webview-based VTCode Chat.
  - `vscode.lm.registerTool` flows (where applicable).

### 2.2 Types: `src/types/toolApproval.ts`

Define shared types for all approval flows.

```ts
// src/types/toolApproval.ts
export type ToolRiskLevel = "low" | "medium" | "high";

export interface ToolApprovalRequest {
    readonly id: string;
    readonly toolId: string;
    readonly toolName: string;
    readonly displayName: string;
    readonly arguments: Record<string, unknown>;
    readonly description?: string;
    readonly previewText?: string;
    readonly riskLevel: ToolRiskLevel;
    readonly estimatedDurationMs?: number;
}

export interface ToolApprovalResponse {
    readonly id: string;
    readonly approved: boolean;
    readonly modifiedArguments?: Record<string, unknown>;
    readonly reason?: string;
    readonly source: "webview" | "inline" | "auto";
}
```

### 2.3 Manager: `src/tools/toolApprovalManager.ts`

Central manager that all tool-executing code uses.

Key responsibilities:

- Accept `ToolApprovalRequest`.
- Apply policy:
  - Workspace trust check.
  - `vtcode.toml` tool policies (allow/prompt/deny).
  - `human_in_the_loop` flag.
- If needed, route request to:
  - Webview UI.
  - Inline VS Code modal fallback.
- Return a `ToolApprovalResponse`.

Implementation sketch:

```ts
// src/tools/toolApprovalManager.ts
import * as vscode from "vscode";
import {
    ToolApprovalRequest,
    ToolApprovalResponse,
    ToolRiskLevel,
} from "../types/toolApproval";
import { VtcodeConfigSummary } from "../vtcodeConfig";

export interface ToolApprovalManagerOptions {
    readonly getConfigSummary: () => VtcodeConfigSummary | undefined;
    readonly isWorkspaceTrusted: () => boolean;
}

export class ToolApprovalManager {
    private pending = new Map<string, (response: ToolApprovalResponse) => void>();
    private webviewPostMessage:
        | ((message: unknown) => void)
        | undefined;

    constructor(private readonly options: ToolApprovalManagerOptions) {}

    attachWebviewSender(sender: (message: unknown) => void): void {
        this.webviewPostMessage = sender;
    }

    async requestApproval(
        request: ToolApprovalRequest,
    ): Promise<ToolApprovalResponse> {
        const policyResponse = this.applyPolicy(request);
        if (policyResponse) {
            return policyResponse;
        }

        if (this.webviewPostMessage) {
            return this.requestViaWebview(request);
        }

        return this.requestInline(request);
    }

    handleWebviewApproval(response: ToolApprovalResponse): void {
        const resolver = this.pending.get(response.id);
        if (!resolver) {
            return;
        }
        this.pending.delete(response.id);
        resolver(response);
    }

    // ---- internal helpers ----

    private applyPolicy(
        request: ToolApprovalRequest,
    ): ToolApprovalResponse | undefined {
        const summary = this.options.getConfigSummary();
        const trusted = this.options.isWorkspaceTrusted();

        // If workspace not trusted, always require explicit approval
        if (!trusted) {
            return undefined;
        }

        // Respect vtcode.toml policies (pseudo-logic; real logic reads summary)
        const hitlEnabled = summary?.humanInTheLoop !== false;

        // Example: if HITL disabled and risk is low, auto-approve
        if (!hitlEnabled && request.riskLevel === "low") {
            return {
                id: request.id,
                approved: true,
                source: "auto",
            };
        }

        // TODO: integrate real tool policy from summary.toolPolicies / toolDefaultPolicy

        return undefined;
    }

    private requestViaWebview(
        request: ToolApprovalRequest,
    ): Promise<ToolApprovalResponse> {
        if (!this.webviewPostMessage) {
            return this.requestInline(request);
        }

        this.webviewPostMessage({
            type: "vtcode.toolApproval.request",
            payload: request,
        });

        return new Promise<ToolApprovalResponse>((resolve) => {
            this.pending.set(request.id, resolve);
        });
    }

    private async requestInline(
        request: ToolApprovalRequest,
    ): Promise<ToolApprovalResponse> {
        const choice = await vscode.window.showWarningMessage(
            this.buildInlineMessage(request),
            { modal: true },
            "Approve",
            "Deny",
        );

        const approved = choice === "Approve";
        return {
            id: request.id,
            approved,
            source: "inline",
        };
    }

    private buildInlineMessage(request: ToolApprovalRequest): string {
        const argsPreview = Object.entries(request.arguments)
            .map(([k, v]) => `${k}: ${JSON.stringify(v)}`)
            .join(", ");
        return `VTCode requests to run tool "${request.displayName}" (${request.toolId}).

Risk: ${request.riskLevel.toUpperCase()}
Args: ${argsPreview}`;
    }
}
```

### 2.4 Webview Integration (ChatView)

In [`src/chatView.ts`](src/chatView.ts):

- Attach `ToolApprovalManager` via `attachWebviewSender`.
- Listen for `vtcode.toolApproval.request` messages in the webview JS:
  - Render modal (as already sketched in improvement docs).
  - On approve/deny, post:
    - `{ type: "vtcode.toolApproval.response", payload: ToolApprovalResponse }`.
- In extension host:
  - On webview message:
    - Call `toolApprovalManager.handleWebviewApproval(response)`.

This keeps:

- All policy + coordination in the extension host.
- All user-facing UI in the webview.

### 2.5 Using the Manager for Tool Calls

Any “dangerous” or “side-effectful” tool execution path should:

1. Construct a `ToolApprovalRequest`.
2. Call `toolApprovalManager.requestApproval(request)`.
3. Only run `VtcodeBackend.executeTool` / `runVtcodeCommand` if approved.

Examples:

- `vtcode-updatePlan` LM tool.
- Future file-editing tools.
- MCP-powered tools.

This replaces ad-hoc confirmation prompts and ensures consistent semantics.

---

## 3. Conversation Persistence

### 3.1 Requirements

- Persist chat threads locally per user/workspace (no server dependency).
- Allow:
  - Multiple threads.
  - Switch/load/delete threads.
- Keep:
  - Sensitive operations gated by trust and policies.
- Avoid:
  - Large or binary data.
  - Breaking current single-conversation behavior.

### 3.2 Types: `src/chat/conversationTypes.ts`

```ts
// src/chat/conversationTypes.ts
export type ChatRole = "user" | "assistant" | "system";

export interface ChatMessage {
    readonly id: string;
    readonly role: ChatRole;
    readonly content: string;
    readonly createdAt: number;
    readonly metadata?: Record<string, unknown>;
}

export interface ConversationThread {
    readonly id: string;
    readonly title: string;
    readonly createdAt: number;
    readonly updatedAt: number;
    readonly messages: ChatMessage[];
    readonly metadata?: Record<string, unknown>;
}
```

### 3.3 Manager: `src/chat/conversationManager.ts`

Follow the pattern from `VSCODE_EXTENSION_CODE_EXAMPLES.md`, tailored to current repo:

- Uses `context.globalStorageUri` (or workspace storage) for persistence.
- JSON per thread: `conversations/{id}.json`.
- APIs:

```ts
// src/chat/conversationManager.ts
import * as vscode from "vscode";
import { ConversationThread, ChatMessage } from "./conversationTypes";

export class ConversationManager implements vscode.Disposable {
    // initialize with storageUri from activate()
    // load/save/list/delete/create as in examples
}
```

Key behaviors:

- `loadThreads()` on activation of chat-related features.
- `createThread(title)` when starting a new conversation.
- `appendMessage(threadId, message)` and persist.
- `deleteThread(threadId)` for cleanup.
- `getThreads()` sorted by `updatedAt`.

### 3.4 ChatView Integration

In `ChatViewProvider`:

- Accept `ConversationManager` via constructor.
- Maintain:
  - `currentThreadId`.
- On:
  - New user message:
    - Append `ChatMessage` to current thread.
  - Assistant/agent reply:
    - Append to same thread.
  - User selects different thread (via webview UI):
    - Load messages from that thread.
- Webview UI:
  - Provide:
    - Thread list (titles).
    - Actions:
      - New thread.
      - Switch thread.
      - Delete thread.
  - Communicate via `postMessage`:
    - `vtcode.conversation.select`, `vtcode.conversation.create`, `vtcode.conversation.delete`.

Rules:

- Default behavior (single-thread) remains:
  - On first run, create “Default” thread.
  - If no conversation features used, behavior matches today.
- If VS Code Chat participant (`vtcode.agent`) is used:
  - Optionally map each LM chat session to a separate thread or reuse “Default”.

---

## 4. Incremental Implementation Order

1. Implement `ToolApprovalManager` + types.
2. Wire `ToolApprovalManager` into `ChatViewProvider`:
   - Only use inline modal at first.
   - Add webview UI once stable.
3. Convert existing high-impact tool flows to use the manager:
   - `vtcode-updatePlan` LM tool.
   - Any CLI-executed tools that modify workspace.
4. Implement `ConversationManager` + types.
5. Wire `ConversationManager` into `ChatViewProvider`:
   - Start with:
     - Single “Default” thread persisted.
   - Then add:
     - Multi-thread UI (if desired) in a backwards-compatible way.
6. Update docs:
   - Reference this plan from:
     - `ARCHITECTURE.md`
     - `COMMAND_AND_PARTICIPANT_REFACTOR_PLAN.md`
   - Ensure behavior matches `VSCODE_EXTENSION_MIGRATION_ROADMAP.md` Phase 3.

---

## 5. Safety & Compatibility

- All tool executions:
  - Must go through:
    - Workspace trust checks.
    - ToolApprovalManager (for anything non-trivial).
- All persistence:
  - Stored in VS Code global/workspace storage.
  - Text-only JSON; no secrets stored beyond existing VS Code/VTCode expectations.
- Existing commands:
  - Keep IDs and observable semantics.
- CLI/web host:
  - For VS Code Web (no CLI), ToolApprovalManager is effectively a no-op for CLI tools (requests fail with clear message).
  - ConversationManager can still function locally.

---

## 6. Summary

This plan:

- Gives VTCode a centralized, extensible Tool Approval system wired to:
  - `vtcode.toml` policy.
  - Workspace trust & HITL.
  - Chat Webview and LM tools.
- Introduces robust Conversation Persistence:
  - Implemented with a small, testable `ConversationManager`.
  - Backwards compatible with current single-session behavior.
- Builds directly on the new architecture and command/participant refactor docs, enabling clean Phase 3 implementation without another deep rewrite.