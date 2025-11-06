import * as vscode from "vscode";
import {
    VtcodeBackend,
    type VtcodeStreamChunk,
    type VtcodeToolCall,
    type VtcodeToolExecutionResult,
} from "./vtcodeBackend";
import { VtcodeConfigSummary } from "./vtcodeConfig";

interface ChatMessage {
    readonly role: "user" | "assistant" | "system" | "tool" | "error";
    readonly content: string;
    readonly timestamp: number;
    readonly metadata?: Record<string, unknown>;
}

interface WebviewMessage {
    readonly type: "ready" | "sendMessage" | "clear" | "cancel" | "retry";
    readonly text?: string;
    readonly messageIndex?: number;
}
export class ChatViewProvider implements vscode.WebviewViewProvider {
    public static readonly viewId = "vtcodeChatView";

    private view: vscode.WebviewView | undefined;
    private readonly messages: ChatMessage[] = [];
    private workspaceTrusted = vscode.workspace.isTrusted;
    private lastHumanInLoopSetting: boolean | undefined;

    constructor(
        private readonly extensionUri: vscode.Uri,
        private readonly backend: VtcodeBackend,
        private readonly output: vscode.OutputChannel
    ) {}

    dispose(): void {
        /* noop */
    }

    public setWorkspaceTrusted(trusted: boolean): void {
        this.workspaceTrusted = trusted;
    }

    public updateConfig(summary: VtcodeConfigSummary | undefined): void {
        const previous = this.lastHumanInLoopSetting;
        this.lastHumanInLoopSetting = summary?.humanInTheLoop ?? undefined;

        if (summary?.humanInTheLoop === false && previous !== false) {
            this.output.appendLine(
                "[chatView] Configuration disabled human_in_the_loop; VTCode prompts will proceed without extra VS Code confirmations."
            );
        }
    }

    public resolveWebviewView(
        view: vscode.WebviewView,
        _context: vscode.WebviewViewResolveContext,
        _token: vscode.CancellationToken
    ): void {
        this.output.appendLine("[chatView] resolveWebviewView called");
        this.view = view;
        view.webview.options = {
            enableScripts: true,
            localResourceRoots: [
                vscode.Uri.joinPath(this.extensionUri, "media"),
            ],
        };

        this.output.appendLine("[chatView] Setting webview HTML");
        view.webview.html = this.getHtml(view.webview);
        this.output.appendLine("[chatView] Webview HTML set successfully");

        view.webview.onDidReceiveMessage(async (message: WebviewMessage) => {
            if (message.type === "ready") {
                this.postTranscript();
                return;
            }

            if (message.type === "clear") {
                this.messages.splice(0, this.messages.length);
                this.postTranscript();
                return;
            }

            if (message.type === "cancel") {
                this.backend.cancelStream();
                this.setThinking(false);
                this.addSystemMessage("Request cancelled by user");
                return;
            }

            if (message.type === "retry") {
                // Find the last user message and retry it
                const lastUserMessage = [...this.messages]
                    .reverse()
                    .find((m) => m.role === "user");
                if (lastUserMessage) {
                    await this.handleUserMessage(lastUserMessage.content);
                }
                return;
            }

            if (message.type === "sendMessage") {
                const text = message.text?.trim();
                if (!text) {
                    return;
                }
                await this.handleUserMessage(text);
            }
        });
    }

    private async handleUserMessage(text: string): Promise<void> {
        if (!(await this.ensureWorkspaceTrusted())) {
            this.addSystemMessage(
                "Trust this workspace to send prompts to VTCode."
            );
            return;
        }

        const contextFlushed = await vscode.commands.executeCommand(
            "vtcode.flushIdeContextSnapshot"
        );
        if (contextFlushed === false) {
            this.output.appendLine(
                "[vtcode] IDE context snapshot is unavailable; continuing without supplemental context."
            );
        }

        const userMessage: ChatMessage = {
            role: "user",
            content: text,
            timestamp: Date.now(),
        };
        this.messages.push(userMessage);
        this.postTranscript();
        this.setThinking(true);

        let assistantContent = "";
        let assistantMetadata: Record<string, unknown> | undefined;
        let reasoningContent = "";
        try {
            // Build conversation context from recent messages
            const context = this.buildConversationContext();

            for await (const chunk of this.backend.streamPrompt({
                prompt: text,
                context,
            })) {
                if (chunk.kind === "text") {
                    assistantContent += chunk.text;
                    this.post({ type: "stream", content: assistantContent });
                } else if (chunk.kind === "reasoning") {
                    reasoningContent += chunk.text;
                    this.post({ type: "reasoning", content: reasoningContent });
                } else if (chunk.kind === "metadata") {
                    assistantMetadata = chunk.metadata;
                } else if (chunk.kind === "toolCall") {
                    await this.handleToolCall(chunk);
                } else if (chunk.kind === "toolResult") {
                    this.addToolSummary(chunk);
                } else if (chunk.kind === "error") {
                    throw new Error(chunk.message);
                } else if (chunk.kind === "done") {
                    break;
                }
            }
            const assistantMessage: ChatMessage = {
                role: "assistant",
                content: assistantContent,
                timestamp: Date.now(),
                metadata: assistantMetadata,
            };
            this.messages.push(assistantMessage);
            this.postTranscript();
        } catch (error) {
            const problem =
                error instanceof Error ? error.message : String(error);
            this.output.appendLine(`[vtcode] Chat request failed: ${problem}`);
            this.addErrorMessage(`Request failed: ${problem}`);
        } finally {
            this.setThinking(false);
        }
    }

    private async handleToolCall(
        chunk: Extract<VtcodeStreamChunk, { kind: "toolCall" }>
    ): Promise<void> {
        const approved = await this.requestToolApproval(chunk.call);
        if (!approved) {
            chunk.reject("User denied tool execution.");
            this.addSystemMessage(
                `Tool ${chunk.call.name} cancelled by the user.`
            );
            return;
        }

        const usesTerminal = this.isTerminalTool(chunk.call.name);
        this.addSystemMessage(`Running tool ${chunk.call.name}...`);

        try {
            const result = usesTerminal
                ? await this.runTerminalCommand(chunk)
                : await this.backend.executeTool(chunk.call);
            const responsePayload =
                result.result !== undefined ? result.result : result.text;
            chunk.respond(responsePayload);
            this.addToolMessage(
                chunk.call,
                result,
                usesTerminal ? "command" : "tool"
            );

            // Show error if exit code is non-zero
            if (result.exitCode && result.exitCode !== 0) {
                this.addErrorMessage(
                    `Tool ${chunk.call.name} exited with code ${result.exitCode}`
                );
            }
        } catch (error) {
            const problem =
                error instanceof Error ? error.message : String(error);
            chunk.reject(problem);
            this.addErrorMessage(`Tool ${chunk.call.name} failed: ${problem}`);
        }
    }

    private async ensureWorkspaceTrusted(): Promise<boolean> {
        if (this.workspaceTrusted) {
            return true;
        }

        const selection = await vscode.window.showWarningMessage(
            "VTCode requires a trusted workspace before processing prompts.",
            { modal: true },
            "Trust Workspace",
            "Cancel"
        );

        if (selection === "Trust Workspace") {
            await vscode.commands.executeCommand("vtcode.trustWorkspace");
            if (vscode.workspace.isTrusted) {
                this.workspaceTrusted = true;
                return true;
            }
        }

        return false;
    }

    private isTerminalTool(toolName: string): boolean {
        const normalized = toolName.toLowerCase();
        return (
            normalized === "run_terminal_cmd" ||
            normalized === "run_shell_command" ||
            normalized === "shell" ||
            normalized === "terminal"
        );
    }

    private async runTerminalCommand(
        chunk: Extract<VtcodeStreamChunk, { kind: "toolCall" }>
    ): Promise<VtcodeToolExecutionResult> {
        const args = chunk.call.args;
        const command = this.extractShellCommand(args);
        if (!command) {
            throw new Error("Tool did not provide a command to run.");
        }

        const cwd =
            this.extractString(args, "cwd") ||
            vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        const shell = this.extractString(args, "shell");

        const result = await this.backend.runPtyCommand(command, {
            cwd,
            shell,
            onData: (chunkText: string) => {
                this.post({ type: "toolStream", chunk: chunkText });
            },
        });
        return result;
    }

    private extractShellCommand(
        args: Record<string, unknown>
    ): string | undefined {
        const candidates = ["command", "cmd", "script", "shell_command", "run"];
        for (const key of candidates) {
            const candidate = this.extractString(args, key);
            if (candidate) {
                return candidate;
            }
        }
        return undefined;
    }

    private extractString(
        source: Record<string, unknown>,
        key: string
    ): string | undefined {
        const value = source?.[key];
        return typeof value === "string" && value.trim().length > 0
            ? value
            : undefined;
    }

    private async requestToolApproval(call: VtcodeToolCall): Promise<boolean> {
        const detail = JSON.stringify(call.args, null, 2);
        const selection = await vscode.window.showInformationMessage(
            `VTCode requested tool "${call.name}".`,
            {
                modal: true,
                detail:
                    detail.length > 1200
                        ? `${detail.slice(0, 1200)}...`
                        : detail,
            },
            { title: "Approve", isCloseAffordance: false },
            { title: "Deny", isCloseAffordance: true }
        );

        const approved = selection?.title === "Approve";
        this.output.appendLine(
            `[vtcode] Tool ${call.name} ${approved ? "approved" : "denied"}.`
        );
        return approved;
    }

    private addSystemMessage(content: string): void {
        const message: ChatMessage = {
            role: "system",
            content,
            timestamp: Date.now(),
        };
        this.messages.push(message);
        this.postTranscript();
    }

    private addErrorMessage(content: string): void {
        const message: ChatMessage = {
            role: "error",
            content,
            timestamp: Date.now(),
        };
        this.messages.push(message);
        this.postTranscript();
    }

    private addToolMessage(
        call: VtcodeToolCall,
        result: VtcodeToolExecutionResult,
        toolKind: "command" | "tool"
    ): void {
        const text = result.text.trim();
        const preview = text.length > 0 ? text : "(no output)";
        const exitSuffix =
            typeof result.exitCode === "number"
                ? ` (exit ${result.exitCode})`
                : "";
        const message: ChatMessage = {
            role: "tool",
            content: `Tool ${call.name}${exitSuffix} result:\n${preview}`,
            timestamp: Date.now(),
            metadata: {
                tool: call.name,
                arguments: call.args,
                rawResult: result.result ?? result.text,
                exitCode: result.exitCode,
                toolType: toolKind,
                humanApproved: true, // Mark as HITL approved
            },
        };
        this.messages.push(message);
        this.postTranscript();
    }

    private addToolSummary(
        summary: Extract<VtcodeStreamChunk, { kind: "toolResult" }>
    ): void {
        const label = summary.toolType === "command" ? "Command" : "Tool";
        const normalizedStatus = summary.status.replace(/_/g, " ");
        const exitSuffix =
            summary.exitCode !== undefined ? ` (exit ${summary.exitCode})` : "";
        const header = `${label} ${summary.name}${exitSuffix} ${normalizedStatus}.`;
        const output = summary.output?.trim();
        const content =
            output && output.length > 0
                ? `${header}
${output}`
                : header;
        const metadata: Record<string, unknown> = {
            toolType: summary.toolType,
            status: summary.status,
            exitCode: summary.exitCode,
            rawEvent: summary.rawEvent,
        };
        if (summary.toolType === "command") {
            metadata.command = summary.name;
        }
        if (summary.toolType === "mcp") {
            metadata.toolName = summary.name;
            metadata.arguments = summary.arguments;
        }

        const message: ChatMessage = {
            role: "tool",
            content,
            timestamp: Date.now(),
            metadata,
        };
        this.messages.push(message);
        this.postTranscript();
    }

    private setThinking(active: boolean): void {
        this.post({ type: "thinking", active });
    }

    private buildConversationContext(): string {
        const hitlState =
            this.lastHumanInLoopSetting === false ? "disabled" : "enabled";
        const toolGuidance =
            this.lastHumanInLoopSetting === false
                ? "Tools may execute without a manual approval prompt; describe safety checks before running destructive commands."
                : "Tools require human approval before execution; propose shell commands and edits explicitly.";
        const preamble = `system: You are the VTCode workspace agent running inside VS Code. Workspace trust is ${
            this.workspaceTrusted ? "granted" : "restricted"
        }. Human-in-the-loop is ${hitlState}. ${toolGuidance} Prefer using VTCode tools or PTY sessions for filesystem or shell access, and reference the IDE context snapshot when available.`;

        const relevantMessages = this.messages
            .filter(
                (message) =>
                    message.role === "user" ||
                    message.role === "assistant" ||
                    message.role === "tool"
            )
            .slice(-12);

        if (relevantMessages.length === 0) {
            return preamble;
        }

        const transcript = relevantMessages
            .map((message) => {
                const role = message.role === "tool" ? "tool" : message.role;
                const content = this.truncateForContext(message.content, 2000);
                return `${role}: ${content}`;
            })
            .join("\n\n");

        return `${preamble}\n\n${transcript}`;
    }

    private truncateForContext(content: string, limit: number): string {
        if (content.length <= limit) {
            return content;
        }
        return `${content.slice(0, limit - 20)}… [truncated]`;
    }

    private postTranscript(): void {
        this.post({ type: "transcript", messages: this.messages });
    }

    private post(payload: unknown): void {
        if (!this.view) {
            return;
        }
        void this.view.webview
            .postMessage(payload)
            .then(undefined, (error: unknown) => {
                const message =
                    error instanceof Error ? error.message : String(error);
                this.output.appendLine(
                    `[vtcode] Failed to post chat update: ${message}`
                );
            });
    }

    private getHtml(webview: vscode.Webview): string {
        const scriptUri = webview.asWebviewUri(
            vscode.Uri.joinPath(this.extensionUri, "media", "chat-view.js")
        );
        const styleUri = webview.asWebviewUri(
            vscode.Uri.joinPath(this.extensionUri, "media", "chat-view.css")
        );

        const nonce = this.createNonce();

        return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8" />
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} https:; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}';" />
<meta name="viewport" content="width=device-width, initial-scale=1.0" />
<link rel="stylesheet" href="${styleUri}" />
<title>VTCode Chat</title>
</head>
<body>
    <div id="chat-root" class="chat-root">
        <div class="chat-surface">
            <header class="chat-header">
                <div class="chat-title">
                    <span class="chat-logo">VT</span>
                    <span>VTCode Companion</span>
                </div>
                <div id="status" class="chat-status" aria-live="polite"></div>
            </header>
            <div id="transcript" class="chat-transcript" role="log" aria-live="polite"></div>
            <form id="composer" class="chat-composer" aria-label="Send a message">
                <textarea id="message" class="chat-input" rows="3" placeholder="Ask VTCode a question"></textarea>
                <div class="chat-actions">
                    <button id="send" type="submit" class="chat-button">Send</button>
                    <button id="cancel" type="button" class="chat-button chat-button--secondary" style="display: none;">Cancel</button>
                    <button id="clear" type="button" class="chat-button chat-button--secondary">Clear</button>
                </div>
            </form>
        </div>
    </div>
	<script nonce="${nonce}" src="${scriptUri}"></script>
</body>
</html>`;
    }

    private createNonce(): string {
        return Math.random().toString(36).slice(2, 10);
    }
}
