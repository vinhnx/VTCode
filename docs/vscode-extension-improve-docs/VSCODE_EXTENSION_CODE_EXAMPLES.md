# VT Code VSCode Extension: Implementation Code Examples

This document provides concrete code examples for implementing the improvements outlined in VSCODE_EXTENSION_IMPROVEMENTS.md.

---

## 1. Chat Participant System

### Interface Definitions

```typescript
// src/types/participant.ts
export interface ChatParticipant {
    readonly id: string;
    readonly displayName: string;
    readonly description?: string;
    readonly icon?: string;
    readonly priority: number; // For ordering in UI

    /** Check if this participant can handle the given context */
    canHandle(context: ParticipantContext): boolean;

    /** Resolve additional context based on message content */
    resolveReferenceContext?(
        message: string,
        context: ParticipantContext
    ): Promise<string>;

    /** Get system prompt additions for this participant */
    getSystemPrompt?(): string;

    /** Handle specific participant commands (e.g., @workspace) */
    handleCommand?(command: string, args: string[]): Promise<string>;
}

export interface ParticipantContext {
    readonly activeFile?: FileContext;
    readonly selectedText?: string;
    readonly workspaceRoot?: string;
    readonly terminalOutput?: string;
    readonly recentFiles?: string[];
}
```

### Workspace Participant Implementation

```typescript
// src/participants/workspaceParticipant.ts
import * as vscode from "vscode";
import { ChatParticipant, ParticipantContext } from "../types/participant";

export class WorkspaceParticipant implements ChatParticipant {
    readonly id = "vtcode.workspace";
    readonly displayName = "Workspace";
    readonly description = "Understand your entire project structure";
    readonly icon = "folder";
    readonly priority = 1;

    constructor(private workspace: vscode.WorkspaceFolder) {}

    canHandle(context: ParticipantContext): boolean {
        // Always available if workspace is open
        return !!context.workspaceRoot;
    }

    async resolveReferenceContext(
        message: string,
        context: ParticipantContext
    ): Promise<string> {
        if (!message.includes("@workspace")) {
            return message;
        }

        const structure = await this.buildProjectStructure();
        const dependencies = await this.extractDependencies();

        return `${message}

## Project Structure
\`\`\`
${structure}
\`\`\`

## Dependencies
${dependencies}`;
    }

    getSystemPrompt(): string {
        return `You have access to the entire workspace structure.
When analyzing code, consider the project architecture and dependencies.
Always provide context-aware suggestions based on the project layout.`;
    }

    private async buildProjectStructure(maxDepth = 3): Promise<string> {
        const pattern = new vscode.RelativePattern(
            this.workspace,
            `**/*.ts,**/*.tsx,**/*.rs,**/*.toml` // Focus on code files
        );
        const files = await vscode.workspace.findFiles(
            pattern,
            "**/node_modules/**",
            50 // Limit for performance
        );

        const tree = new Map<string, string[]>();
        files.forEach((file) => {
            const dir = file.fsPath.split("/").slice(0, -1).join("/");
            if (!tree.has(dir)) {
                tree.set(dir, []);
            }
            tree.get(dir)!.push(file.fsPath.split("/").pop()!);
        });

        return Array.from(tree.entries())
            .slice(0, 20)
            .map(([dir, files]) => `${dir}\n  ${files.join("\n  ")}`)
            .join("\n");
    }

    private async extractDependencies(): Promise<string> {
        const patterns = [
            new vscode.RelativePattern(this.workspace, "package.json"),
            new vscode.RelativePattern(this.workspace, "Cargo.toml"),
            new vscode.RelativePattern(this.workspace, "pyproject.toml"),
        ];

        const deps: string[] = [];
        for (const pattern of patterns) {
            const [file] = await vscode.workspace.findFiles(pattern);
            if (file) {
                const doc = await vscode.workspace.openTextDocument(file);
                deps.push(
                    `- **${file.fsPath.split("/").pop()}**:\n${doc.getText()}`
                );
            }
        }

        return deps.join("\n\n");
    }
}
```

### Code Participant Implementation

```typescript
// src/participants/codeParticipant.ts
export class CodeParticipant implements ChatParticipant {
    readonly id = "vtcode.code";
    readonly displayName = "Code";
    readonly description = "Reference specific code files and selections";
    readonly icon = "symbol-file";
    readonly priority = 2;

    async resolveReferenceContext(
        message: string,
        context: ParticipantContext
    ): Promise<string> {
        // Find #file references
        const fileMatches = message.match(/#[\w.\/-]+/g) || [];

        let augmented = message;
        for (const match of fileMatches) {
            const filePath = match.slice(1);
            const fileUri = await this.findFileInWorkspace(filePath);
            if (fileUri) {
                const content = await this.readFileWithContext(fileUri);
                augmented += `\n\n## File: ${filePath}\n${content}`;
            }
        }

        // Include active file if @code mentioned
        if (message.includes("@code") && context.activeFile) {
            augmented += `\n\n## Active File: ${context.activeFile.path}\n\`\`\`${context.activeFile.language}\n${context.activeFile.content}\n\`\`\``;
        }

        return augmented;
    }

    private async findFileInWorkspace(
        filePath: string
    ): Promise<vscode.Uri | undefined> {
        const [match] = await vscode.workspace.findFiles(
            `**/${filePath}`,
            "**/node_modules/**",
            1
        );
        return match;
    }

    private async readFileWithContext(uri: vscode.Uri): Promise<string> {
        const doc = await vscode.workspace.openTextDocument(uri);
        const content = doc.getText();

        // Limit size for performance
        if (content.length > 5000) {
            return `${content.slice(0, 5000)}...\n\n// Content truncated (${
                content.length
            } chars total)`;
        }
        return `\`\`\`${doc.languageId}\n${content}\n\`\`\``;
    }

    canHandle(): boolean {
        return true; // Always available
    }
}
```

### Participant Registry

```typescript
// src/participants/participantRegistry.ts
export class ParticipantRegistry {
    private participants = new Map<string, ChatParticipant>();
    private onParticipantsChanged = new vscode.EventEmitter<void>();

    readonly onDidChange = this.onParticipantsChanged.event;

    register(participant: ChatParticipant): void {
        this.participants.set(participant.id, participant);
        this.onParticipantsChanged.fire();
    }

    unregister(id: string): void {
        this.participants.delete(id);
        this.onParticipantsChanged.fire();
    }

    getParticipants(): ChatParticipant[] {
        return Array.from(this.participants.values()).sort(
            (a, b) => a.priority - b.priority
        );
    }

    async resolveMessage(
        message: string,
        context: ParticipantContext
    ): Promise<string> {
        let resolved = message;

        // Apply participant context resolution in priority order
        for (const participant of this.getParticipants()) {
            if (
                participant.canHandle(context) &&
                participant.resolveReferenceContext
            ) {
                try {
                    resolved = await participant.resolveReferenceContext(
                        resolved,
                        context
                    );
                } catch (error) {
                    console.error(
                        `Participant ${participant.id} failed:`,
                        error
                    );
                }
            }
        }

        return resolved;
    }

    dispose(): void {
        this.participants.clear();
        this.onParticipantsChanged.dispose();
    }
}
```

---

## 2. Command System Refactoring

### Command Interface

```typescript
// src/types/command.ts
export interface CommandContext {
    readonly extension: vscode.ExtensionContext;
    readonly output: vscode.OutputChannel;
    readonly backend: VtcodeBackend;
    readonly workspace: vscode.WorkspaceFolder | undefined;
}

export interface ICommand {
    readonly id: string;
    readonly title: string;
    readonly description?: string;
    readonly icon?: string;
    readonly category: string;
    readonly keybinding?: string;

    execute(context: CommandContext, ...args: unknown[]): Promise<void>;
    canExecute?(context: CommandContext): boolean;
}
```

### Ask Command Implementation

```typescript
// src/commands/askCommand.ts
import { ICommand, CommandContext } from "../types/command";

export class AskCommand implements ICommand {
    readonly id = "vtcode.askAgent";
    readonly title = "Ask the Agent";
    readonly description = "Send a question to the VT Code agent";
    readonly icon = "comment-discussion";
    readonly category = "VT Code";
    readonly keybinding = "ctrl+shift+i";

    async execute(context: CommandContext): Promise<void> {
        const question = await vscode.window.showInputBox({
            prompt: "What would you like VT Code to help with?",
            placeHolder: "e.g., Explain this function",
            ignoreFocusOut: true,
            title: this.title,
        });

        if (!question?.trim()) {
            return;
        }

        try {
            const promptWithContext = await this.appendIdeContext(
                question,
                context
            );

            context.output.appendLine(`[ask] Sending: ${question}`);

            await this.executeVtcodeCommand(
                context,
                ["ask", promptWithContext],
                "Asking VT Code…"
            );

            void vscode.window.showInformationMessage(
                "VT Code finished processing your request. Check the output channel for details."
            );
        } catch (error) {
            const message =
                error instanceof Error ? error.message : String(error);
            void vscode.window.showErrorMessage(`Failed to ask: ${message}`);
        }
    }

    canExecute(context: CommandContext): boolean {
        return vscode.workspace.isTrusted;
    }

    private async appendIdeContext(
        text: string,
        context: CommandContext
    ): Promise<string> {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            return text;
        }

        const file = editor.document.fileName;
        const language = editor.document.languageId;
        const range = editor.selection;

        return `${text}

File: ${file}
Language: ${language}
Range: lines ${range.start.line + 1}-${range.end.line + 1}`;
    }

    private async executeVtcodeCommand(
        context: CommandContext,
        args: string[],
        title: string
    ): Promise<void> {
        // Use the shared execution utility
        await vscode.window.withProgress(
            { location: vscode.ProgressLocation.Notification, title },
            async () => {
                // Implementation
            }
        );
    }
}
```

### Ask Selection Command

```typescript
// src/commands/askSelectionCommand.ts
export class AskSelectionCommand implements ICommand {
    readonly id = "vtcode.askSelection";
    readonly title = "Ask About Selection";
    readonly description = "Ask VT Code about the selected code";
    readonly icon = "comment";
    readonly category = "VT Code";

    async execute(context: CommandContext): Promise<void> {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            void vscode.window.showWarningMessage(
                "Open a text editor to ask about the selection."
            );
            return;
        }

        if (editor.selection.isEmpty) {
            void vscode.window.showWarningMessage(
                "Select code first, then run this command."
            );
            return;
        }

        const selectedText = editor.document.getText(editor.selection);
        if (!selectedText.trim()) {
            void vscode.window.showWarningMessage(
                "The selected text is empty."
            );
            return;
        }

        const question = await vscode.window.showInputBox({
            prompt: "What would you like to know?",
            value: "Explain this code",
            ignoreFocusOut: true,
        });

        if (!question?.trim()) {
            return;
        }

        try {
            const prompt = this.buildPrompt(
                question,
                selectedText,
                editor.document,
                editor.selection
            );

            await vscode.commands.executeCommand("vtcode.askAgent", prompt);
        } catch (error) {
            const message =
                error instanceof Error ? error.message : String(error);
            void vscode.window.showErrorMessage(`Failed: ${message}`);
        }
    }

    canExecute(): boolean {
        return (
            !!vscode.window.activeTextEditor &&
            !vscode.window.activeTextEditor.selection.isEmpty &&
            vscode.workspace.isTrusted
        );
    }

    private buildPrompt(
        question: string,
        selectedText: string,
        document: vscode.TextDocument,
        range: vscode.Selection
    ): string {
        const language = document.languageId;
        const startLine = range.start.line + 1;
        const endLine = range.end.line + 1;
        const file = vscode.workspace.asRelativePath(document.uri);

        return `${question}

## Selected Code
File: \`${file}\`
Lines: ${startLine}-${endLine}

\`\`\`${language}
${selectedText}
\`\`\``;
    }
}
```

### Command Registry

```typescript
// src/commandRegistry.ts
export class CommandRegistry {
  private commands = new Map<string, ICommand>()

  register(command: ICommand, context: vscode.ExtensionContext): void {
    this.commands.set(command.id, command)

    const commandContext: CommandContext = {
      extension: context,
      output: vscode.window.createOutputChannel("VT Code"),
      backend: new VtcodeBackend(...),
      workspace: vscode.workspace.workspaceFolders?.[0],
    }

    context.subscriptions.push(
      vscode.commands.registerCommand(command.id, async (...args) => {
        if (command.canExecute && !command.canExecute(commandContext)) {
          void vscode.window.showWarningMessage(
            `${command.title} is not available in this context.`
          )
          return
        }

        try {
          await command.execute(commandContext, ...args)
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error)
          void vscode.window.showErrorMessage(
            `${command.title} failed: ${message}`
          )
        }
      })
    )
  }

  getCommand(id: string): ICommand | undefined {
    return this.commands.get(id)
  }

  getAllCommands(): ICommand[] {
    return Array.from(this.commands.values())
  }
}
```

---

## 3. Tool Approval UI

### Approval Request Types

```typescript
// src/types/toolApproval.ts
export interface ToolApprovalRequest {
    readonly id: string;
    readonly toolId: string;
    readonly toolName: string;
    readonly displayName: string;
    readonly arguments: Record<string, unknown>;
    readonly description?: string;
    readonly previewText?: string;
    readonly riskLevel: "low" | "medium" | "high";
    readonly estimatedDuration?: number;
}

export interface ToolApprovalResponse {
    readonly approved: boolean;
    readonly modifiedArguments?: Record<string, unknown>;
    readonly reason?: string;
}

export interface ToolExecutionProgress {
    readonly id: string;
    readonly status: "pending" | "running" | "completed" | "failed";
    readonly progress?: number;
    readonly message?: string;
    readonly output?: string;
    readonly startTime?: number;
    readonly endTime?: number;
}
```

### Tool Approval Manager

```typescript
// src/tools/toolApprovalManager.ts
export class ToolApprovalManager implements vscode.Disposable {
    private pendingApprovals = new Map<string, ToolApprovalRequest>();
    private onApprovalNeeded = new vscode.EventEmitter<ToolApprovalRequest>();
    private onApprovalResponse =
        new vscode.EventEmitter<ToolApprovalResponse>();

    readonly approvalNeeded = this.onApprovalNeeded.event;
    readonly approvalResponse = this.onApprovalResponse.event;

    async requestApproval(
        request: ToolApprovalRequest
    ): Promise<ToolApprovalResponse> {
        this.pendingApprovals.set(request.id, request);
        this.onApprovalNeeded.fire(request);

        return new Promise((resolve) => {
            const listener = this.onApprovalResponse.event((response) => {
                if (response.id === request.id) {
                    listener.dispose();
                    resolve(response);
                }
            });
        });
    }

    async respondToApproval(response: ToolApprovalResponse): Promise<void> {
        this.pendingApprovals.delete(response.id);
        this.onApprovalResponse.fire(response);
    }

    getPendingApprovals(): ToolApprovalRequest[] {
        return Array.from(this.pendingApprovals.values());
    }

    dispose(): void {
        this.pendingApprovals.clear();
        this.onApprovalNeeded.dispose();
        this.onApprovalResponse.dispose();
    }
}
```

### Chat View Integration

```typescript
// In src/chatView.ts - Tool approval UI section

private async handleToolCall(call: VtcodeToolCall): Promise<VtcodeToolExecutionResult> {
  // Create approval request
  const request: ToolApprovalRequest = {
    id: call.id,
    toolId: call.name,
    toolName: call.name,
    displayName: this.getToolDisplayName(call.name),
    arguments: call.args,
    description: this.getToolDescription(call.name),
    riskLevel: this.assessRisk(call.name, call.args),
    previewText: this.buildToolPreview(call),
  }

  // Request approval from UI
  const response = await this.approvalManager.requestApproval(request)

  if (!response.approved) {
    return {
      text: `Tool execution denied: ${response.reason || "User declined"}`,
      exitCode: 1,
    }
  }

  // Execute tool with approved arguments
  const finalArgs = response.modifiedArguments ?? call.args

  try {
    // Show execution progress in chat
    this.showToolExecutionStarted(call.name)

    const result = await this.backend.executeTool(call.name, finalArgs)

    this.showToolExecutionResult(call.name, result)
    return result
  } catch (error) {
    const errorText = error instanceof Error ? error.message : String(error)
    this.showToolExecutionError(call.name, errorText)
    return { text: errorText, exitCode: 1 }
  }
}

private buildToolPreview(call: VtcodeToolCall): string {
  const argsList = Object.entries(call.args)
    .map(([k, v]) => `  - ${k}: ${JSON.stringify(v)}`)
    .join("\n")

  return `Tool: **${call.name}**\n\nArguments:\n${argsList}`
}

private assessRisk(toolName: string, _args: Record<string, unknown>): ToolRiskLevel {
  const dangerousTools = ["delete", "rm", "chmod", "kill"]
  if (dangerousTools.some(t => toolName.includes(t))) {
    return "high"
  }

  const moderateTools = ["create", "write", "modify", "rename"]
  if (moderateTools.some(t => toolName.includes(t))) {
    return "medium"
  }

  return "low"
}

private showToolExecutionStarted(toolName: string): void {
  this.addSystemMessage(`⏳ Starting tool execution: **${toolName}**`)
  this.setThinking(true)
}

private showToolExecutionResult(
  toolName: string,
  result: VtcodeToolExecutionResult
): void {
  const status = result.exitCode === 0 ? " " : " "
  this.addSystemMessage(
    `${status} Tool execution completed: **${toolName}**\n\n\`\`\`\n${result.text}\n\`\`\``
  )
}

private showToolExecutionError(toolName: string, error: string): void {
  this.addSystemMessage(
    `  Tool execution failed: **${toolName}**\n\n\`\`\`\n${error}\n\`\`\``
  )
}
```

### Webview HTML for Approval (media/chatView.html)

```html
<!-- Tool approval modal section -->
<div id="approval-modal" class="modal hidden">
    <div class="modal-content">
        <div class="approval-header">
            <h3>Tool Execution Approval</h3>
            <p class="approval-description" id="approval-description"></p>
        </div>

        <div class="tool-preview">
            <h4>Tool Details</h4>
            <div id="tool-preview-content"></div>
        </div>

        <div class="approval-actions">
            <button class="btn btn-success" id="approve-btn">
                Approve & Execute
            </button>
            <button class="btn btn-danger" id="deny-btn">Deny</button>
            <button class="btn btn-default" id="modify-btn">
                Review Arguments
            </button>
        </div>

        <div class="risk-indicator" id="risk-indicator"></div>
    </div>
</div>

<style>
    .modal {
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        background: rgba(0, 0, 0, 0.5);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 1000;
    }

    .modal.hidden {
        display: none;
    }

    .modal-content {
        background: var(--vscode-editor-background);
        border: 1px solid var(--vscode-panel-border);
        border-radius: 8px;
        padding: 20px;
        max-width: 600px;
        max-height: 80vh;
        overflow-y: auto;
        box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3);
    }

    .approval-header h3 {
        margin: 0 0 10px 0;
        color: var(--vscode-editor-foreground);
    }

    .tool-preview {
        background: var(--vscode-editor-inlineValue-background);
        padding: 12px;
        border-radius: 4px;
        margin: 15px 0;
        font-family: monospace;
        font-size: 12px;
        border-left: 3px solid var(--vscode-inputValidation-infoBackground);
    }

    .approval-actions {
        display: flex;
        gap: 10px;
        margin-top: 20px;
    }

    .btn {
        flex: 1;
        padding: 10px;
        border: none;
        border-radius: 4px;
        font-size: 14px;
        cursor: pointer;
        transition: opacity 0.2s;
    }

    .btn:hover {
        opacity: 0.8;
    }

    .btn-success {
        background: var(--vscode-statusBar-successBackground);
        color: var(--vscode-statusBar-successForeground);
    }

    .btn-danger {
        background: var(--vscode-errorForeground);
        color: white;
    }

    .btn-default {
        background: var(--vscode-button-secondaryBackground);
        color: var(--vscode-button-secondaryForeground);
    }

    .risk-indicator {
        margin-top: 15px;
        padding: 10px;
        border-radius: 4px;
        font-size: 13px;
    }

    .risk-high {
        background: var(--vscode-inputValidation-errorBackground);
        color: var(--vscode-inputValidation-errorForeground);
        border: 1px solid var(--vscode-inputValidation-errorBorder);
    }

    .risk-medium {
        background: var(--vscode-inputValidation-warningBackground);
        color: var(--vscode-inputValidation-warningForeground);
        border: 1px solid var(--vscode-inputValidation-warningBorder);
    }

    .risk-low {
        background: var(--vscode-inputValidation-infoBackground);
        color: var(--vscode-inputValidation-infoForeground);
        border: 1px solid var(--vscode-inputValidation-infoBorder);
    }
</style>
```

---

## 4. Conversation Persistence

```typescript
// src/chat/conversationManager.ts
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs/promises";

export interface ConversationThread {
    readonly id: string;
    readonly title: string;
    readonly created: number;
    readonly lastModified: number;
    readonly messages: ChatMessage[];
    readonly metadata?: Record<string, unknown>;
}

export class ConversationManager implements vscode.Disposable {
    private threads = new Map<string, ConversationThread>();
    private storageUri: vscode.Uri;
    private onThreadsChanged = new vscode.EventEmitter<void>();

    readonly threadsChanged = this.onThreadsChanged.event;

    constructor(storageUri: vscode.Uri) {
        this.storageUri = vscode.Uri.joinPath(storageUri, "conversations");
    }

    async loadThreads(): Promise<void> {
        try {
            await vscode.workspace.fs.createDirectory(this.storageUri);
            const files = await vscode.workspace.fs.readDirectory(
                this.storageUri
            );

            for (const [name, type] of files) {
                if (type === vscode.FileType.File && name.endsWith(".json")) {
                    try {
                        const data = await vscode.workspace.fs.readFile(
                            vscode.Uri.joinPath(this.storageUri, name)
                        );
                        const thread = JSON.parse(
                            Buffer.from(data).toString("utf8")
                        );
                        this.threads.set(thread.id, thread);
                    } catch (error) {
                        console.error(`Failed to load thread ${name}:`, error);
                    }
                }
            }

            this.onThreadsChanged.fire();
        } catch (error) {
            console.error("Failed to load conversation threads:", error);
        }
    }

    async saveThread(thread: ConversationThread): Promise<void> {
        const updated: ConversationThread = {
            ...thread,
            lastModified: Date.now(),
        };

        this.threads.set(thread.id, updated);

        const filePath = vscode.Uri.joinPath(
            this.storageUri,
            `${thread.id}.json`
        );

        try {
            const data = Buffer.from(JSON.stringify(updated, null, 2), "utf8");
            await vscode.workspace.fs.writeFile(filePath, data);
            this.onThreadsChanged.fire();
        } catch (error) {
            console.error(`Failed to save thread ${thread.id}:`, error);
            throw error;
        }
    }

    async deleteThread(id: string): Promise<void> {
        const filePath = vscode.Uri.joinPath(this.storageUri, `${id}.json`);

        try {
            await vscode.workspace.fs.delete(filePath);
            this.threads.delete(id);
            this.onThreadsChanged.fire();
        } catch (error) {
            console.error(`Failed to delete thread ${id}:`, error);
            throw error;
        }
    }

    getThreads(): ConversationThread[] {
        return Array.from(this.threads.values()).sort(
            (a, b) => b.lastModified - a.lastModified
        );
    }

    getThread(id: string): ConversationThread | undefined {
        return this.threads.get(id);
    }

    createThread(title: string): ConversationThread {
        const id = `thread-${Date.now()}-${Math.random()
            .toString(36)
            .slice(2)}`;
        const thread: ConversationThread = {
            id,
            title,
            created: Date.now(),
            lastModified: Date.now(),
            messages: [],
        };
        return thread;
    }

    dispose(): void {
        this.threads.clear();
        this.onThreadsChanged.dispose();
    }
}
```

---

## 5. Status Indicators

```typescript
// src/ui/statusIndicators.ts
export interface ChatUIMetrics {
    readonly status: "idle" | "thinking" | "streaming" | "executing" | "error";
    readonly elapsedMs?: number;
    readonly tokensUsed?: number;
    readonly modelName?: string;
    readonly participantName?: string;
    readonly toolName?: string;
}

export class StatusIndicator {
    private metrics: ChatUIMetrics = { status: "idle" };
    private startTime = 0;

    update(metrics: Partial<ChatUIMetrics>): void {
        this.metrics = { ...this.metrics, ...metrics };

        if (metrics.status === "streaming" || metrics.status === "thinking") {
            this.startTime = Date.now();
        }

        this.render();
    }

    private render(): void {
        const indicators = this.buildIndicators();
        this.updateUI(indicators);
    }

    private buildIndicators(): string {
        const parts: string[] = [];

        // Status icon
        switch (this.metrics.status) {
            case "idle":
                parts.push("");
                break;
            case "thinking":
                parts.push("");
                break;
            case "streaming":
                parts.push("");
                break;
            case "executing":
                parts.push("");
                break;
            case "error":
                parts.push("");
                break;
        }

        // Status text
        parts.push(this.metrics.status.toUpperCase());

        // Elapsed time
        if (this.metrics.elapsedMs) {
            parts.push(`• ${(this.metrics.elapsedMs / 1000).toFixed(1)}s`);
        }

        // Tokens
        if (this.metrics.tokensUsed) {
            parts.push(`• ${this.metrics.tokensUsed} tokens`);
        }

        // Model
        if (this.metrics.modelName) {
            parts.push(`• ${this.metrics.modelName}`);
        }

        // Participant
        if (this.metrics.participantName) {
            parts.push(`@${this.metrics.participantName}`);
        }

        // Tool
        if (this.metrics.toolName) {
            parts.push(` ${this.metrics.toolName}`);
        }

        return parts.join(" ");
    }

    private updateUI(text: string): void {
        // Send to webview
        // Implementation depends on how status is displayed
    }
}
```

---

## 6. Enhanced Error Handling

```typescript
// src/error/errorRecovery.ts
export interface ErrorRecovery {
    readonly condition: (error: Error) => boolean;
    readonly recover: (context: ErrorContext) => Promise<void>;
    readonly userMessage: string;
    readonly logLevel: "error" | "warn" | "info";
    readonly isRecoverable: boolean;
}

export class ErrorRecoveryHandler {
    private recoveryStrategies: ErrorRecovery[] = [
        {
            condition: (e) => e.message.includes("timeout"),
            recover: (ctx) => this.handleTimeout(ctx),
            userMessage:
                "Request timed out. Retrying with a shorter context window.",
            logLevel: "warn",
            isRecoverable: true,
        },
        {
            condition: (e) => e.message.includes("rate limit"),
            recover: (ctx) => this.handleRateLimit(ctx),
            userMessage: "Rate limited. Waiting before retrying...",
            logLevel: "warn",
            isRecoverable: true,
        },
        {
            condition: (e) => e.message.includes("token"),
            recover: (ctx) => this.handleTokenLimit(ctx),
            userMessage:
                "Token limit exceeded. Summarizing context and continuing.",
            logLevel: "warn",
            isRecoverable: true,
        },
    ];

    async handleError(error: Error, context: ErrorContext): Promise<boolean> {
        const strategy = this.recoveryStrategies.find((s) =>
            s.condition(error)
        );

        if (!strategy || !strategy.isRecoverable) {
            return false;
        }

        try {
            context.output.appendLine(
                `[${strategy.logLevel}] ${strategy.userMessage}`
            );
            await strategy.recover(context);
            return true;
        } catch (recoveryError) {
            context.output.appendLine(
                `[error] Recovery failed: ${recoveryError}`
            );
            return false;
        }
    }

    private async handleTimeout(context: ErrorContext): Promise<void> {
        // Reduce context size
        context.contextSize = Math.floor((context.contextSize || 100) / 2);
        // Retry with exponential backoff
        await this.delay(2000);
    }

    private async handleRateLimit(context: ErrorContext): Promise<void> {
        // Wait with exponential backoff
        const wait = 1000 * Math.pow(2, context.retryCount || 0);
        await this.delay(Math.min(wait, 30000));
        context.retryCount = (context.retryCount || 0) + 1;
    }

    private async handleTokenLimit(context: ErrorContext): Promise<void> {
        // Summarize older messages
        const messages = context.messages;
        if (messages.length > 5) {
            const oldMessages = messages.slice(0, -3);
            const summary = await context.backend.summarizeMessages(
                oldMessages
            );
            context.messages = [
                {
                    role: "system",
                    content: `Summary of earlier discussion: ${summary}`,
                },
                ...messages.slice(-3),
            ];
        }
    }

    private delay(ms: number): Promise<void> {
        return new Promise((resolve) => setTimeout(resolve, ms));
    }
}

export interface ErrorContext {
    readonly output: vscode.OutputChannel;
    readonly backend: VtcodeBackend;
    readonly messages: ChatMessage[];
    readonly retryCount?: number;
    contextSize?: number;
}
```

---

## Usage in extension.ts

```typescript
// Example: Integrating new systems in extension.ts

async function activate(context: vscode.ExtensionContext) {
    const outputChannel = vscode.window.createOutputChannel("VT Code");

    // Initialize new systems
    const conversationManager = new ConversationManager(
        context.globalStorageUri
    );
    await conversationManager.loadThreads();

    const participantRegistry = new ParticipantRegistry();
    participantRegistry.register(
        new WorkspaceParticipant(vscode.workspace.workspaceFolders?.[0]!)
    );
    participantRegistry.register(new CodeParticipant());

    const commandRegistry = new CommandRegistry();
    commandRegistry.register(new AskCommand(), context);
    commandRegistry.register(new AskSelectionCommand(), context);

    const toolApprovalManager = new ToolApprovalManager();

    // Update ChatViewProvider
    const chatViewProvider = new ChatViewProvider(
        context.extensionUri,
        chatBackend,
        outputChannel,
        {
            conversationManager,
            participantRegistry,
            toolApprovalManager,
        }
    );

    context.subscriptions.push(
        vscode.window.registerWebviewViewProvider(
            "vtcodeChatView",
            chatViewProvider
        ),
        conversationManager,
        participantRegistry,
        toolApprovalManager
    );
}
```

---

## Testing Examples

```typescript
// tests/unit/participants/workspaceParticipant.test.ts
import { describe, it, expect, beforeEach } from "vitest";
import { WorkspaceParticipant } from "../../../src/participants/workspaceParticipant";

describe("WorkspaceParticipant", () => {
    let participant: WorkspaceParticipant;
    let mockWorkspace: vscode.WorkspaceFolder;

    beforeEach(() => {
        mockWorkspace = {
            uri: vscode.Uri.file("/test/workspace"),
            name: "test-workspace",
            index: 0,
        };
        participant = new WorkspaceParticipant(mockWorkspace);
    });

    it("should handle workspace context", async () => {
        const context = {
            workspaceRoot: "/test/workspace",
            activeFile: undefined,
            selectedText: undefined,
            terminalOutput: undefined,
            recentFiles: [],
        };

        expect(participant.canHandle(context)).toBe(true);
    });

    it("should resolve @workspace references", async () => {
        const message = "What's the project structure? @workspace";
        const context = { workspaceRoot: "/test/workspace" };

        const resolved = await participant.resolveReferenceContext(
            message,
            context
        );

        expect(resolved).toContain("Project Structure");
        expect(resolved).toContain("@workspace").toBeFalsy(); // Should be replaced
    });
});

// tests/unit/commands/askCommand.test.ts
describe("AskCommand", () => {
    it("should execute with valid input", async () => {
        const command = new AskCommand();
        const context: CommandContext = {
            // Mock context
        };

        await command.execute(context);
        // Assert results
    });

    it("should show warning when workspace not trusted", async () => {
        const command = new AskCommand();
        expect(command.canExecute(untrustedContext)).toBe(false);
    });
});
```

---

## Summary

These code examples provide a foundation for implementing the improvements. Key patterns:

1. **Participant System**: Extensible, composable context providers
2. **Command Registry**: Unified, type-safe command handling
3. **Tool Approval**: User-friendly approval workflow with progress tracking
4. **Conversation Persistence**: Thread-based storage with easy management
5. **Error Recovery**: Graceful, automatic error handling
6. **Testing**: Comprehensive test coverage with mocks

All examples follow TypeScript best practices and VS Code API conventions.
