/**
 * Chat View Provider for VTCode
 *
 * Provides a webview-based chat interface with full conversation loop,
 * PTY terminal integration, tool invocation, and human-in-the-loop capabilities.
 */

import * as vscode from "vscode";
import { VtcodeTerminalManager } from "./agentTerminal";

export interface ChatMessage {
    role: "user" | "assistant" | "system" | "tool";
    content: string;
    timestamp: number;
    metadata?: {
        toolCall?: ToolCall;
        toolResult?: ToolResult;
        reasoning?: string;
    };
}

export interface ToolCall {
    id: string;
    name: string;
    arguments: Record<string, unknown>;
}

export interface ToolResult {
    id: string;
    name: string;
    result: unknown;
    error?: string;
    executionTimeMs?: number;
}

export interface TranscriptEntry extends ChatMessage {
    id: string;
}

export class ChatViewProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = "vtcodeChat";
    protected view?: vscode.WebviewView;
    protected transcript: TranscriptEntry[] = [];
    protected messageIdCounter = 0;
    protected pendingApprovals = new Map<string, (approved: boolean) => void>();

    constructor(
        protected readonly context: vscode.ExtensionContext,
        protected readonly terminalManager: VtcodeTerminalManager
    ) {}

    public resolveWebviewView(
        webviewView: vscode.WebviewView,
        _context: vscode.WebviewViewResolveContext,
        _token: vscode.CancellationToken
    ) {
        this.view = webviewView;

        webviewView.webview.options = {
            enableScripts: true,
            localResourceRoots: [this.context.extensionUri],
        };

        webviewView.webview.html = this.getHtmlForWebview(webviewView.webview);

        // Send welcome message after a short delay to ensure webview is ready
        setTimeout(() => {
            this.sendSystemMessage(
                "Welcome to VTCode Chat! Type a message or use /help for commands.",
                "info"
            );
        }, 100);

        // Handle messages from the webview
        webviewView.webview.onDidReceiveMessage(async (data) => {
            switch (data.type) {
                case "userMessage":
                    await this.handleUserMessage(data.text);
                    break;
                case "clearTranscript":
                    this.clearTranscript();
                    break;
                case "toolApproval":
                    this.handleToolApproval(data.toolId, data.approved);
                    break;
                case "cancelOperation":
                    this.handleCancelOperation();
                    break;
                case "executeCommand":
                    await this.handleCommandExecution(data.command);
                    break;
            }
        });
    }
    private async handleUserMessage(text: string): Promise<void> {
        if (!text.trim()) {
            return;
        }

        // Check for special command prefixes
        if (text.startsWith("/")) {
            await this.handleSystemCommand(text);
            return;
        }

        if (text.startsWith("@")) {
            await this.handleAgentCommand(text);
            return;
        }

        if (text.startsWith("#")) {
            await this.handleToolCommand(text);
            return;
        }

        // Regular user message
        const message: ChatMessage = {
            role: "user",
            content: text,
            timestamp: Date.now(),
        };

        this.addToTranscript(message);
        await this.processAgentResponse(text);
    }

    protected async handleSystemCommand(command: string): Promise<void> {
        const cmd = command.slice(1).trim().toLowerCase();
        const args = cmd.split(/\s+/);
        const baseCmd = args[0];

        switch (baseCmd) {
            case "clear":
                this.clearTranscript();
                this.sendSystemMessage("Transcript cleared");
                break;

            case "help":
                this.sendSystemMessage(this.getHelpText());
                break;

            case "export":
                await this.exportTranscript();
                break;

            case "stats":
                this.showStats();
                break;

            case "config":
                await this.showConfig();
                break;

            default:
                this.sendSystemMessage(
                    `Unknown command: /${baseCmd}. Type /help for available commands.`
                );
        }
    }

    protected async handleAgentCommand(command: string): Promise<void> {
        const cmd = command.slice(1).trim();
        const args = cmd.split(/\s+/);
        const baseCmd = args[0];

        this.sendSystemMessage(`Executing agent command: @${baseCmd}`);

        switch (baseCmd) {
            case "analyze":
                await this.analyzeCode();
                break;

            case "explain":
                await this.explainSelection();
                break;

            case "refactor":
                await this.refactorCode();
                break;

            case "test":
                await this.generateTests();
                break;

            default:
                this.sendSystemMessage(`Unknown agent command: @${baseCmd}`);
        }
    }

    private async handleToolCommand(command: string): Promise<void> {
        const cmd = command.slice(1).trim();
        const args = cmd.split(/\s+/);
        const toolName = args[0];

        this.sendSystemMessage(`Invoking tool: #${toolName}`);

        // Parse tool arguments (simple key=value format)
        const toolArgs: Record<string, string> = {};
        for (let i = 1; i < args.length; i++) {
            const [key, value] = args[i].split("=");
            if (key && value) {
                toolArgs[key] = value;
            }
        }

        await this.invokeTool(toolName, toolArgs);
    }

    private async processAgentResponse(userInput: string): Promise<void> {
        // Show thinking indicator
        this.sendThinkingIndicator(true);

        try {
            // Simulate agent processing (in real implementation, call vtcode CLI)
            const response = await this.callVtcodeAgent(userInput);

            // Add assistant response to transcript
            const message: ChatMessage = {
                role: "assistant",
                content: response.content,
                timestamp: Date.now(),
                metadata: response.metadata,
            };

            this.addToTranscript(message);

            // Handle tool calls if any
            if (response.toolCalls && response.toolCalls.length > 0) {
                await this.handleToolCalls(response.toolCalls);
            }
        } catch (error) {
            this.sendSystemMessage(
                `Error: ${
                    error instanceof Error ? error.message : String(error)
                }`,
                "error"
            );
        } finally {
            this.sendThinkingIndicator(false);
        }
    }

    private async callVtcodeAgent(input: string): Promise<{
        content: string;
        metadata?: ChatMessage["metadata"];
        toolCalls?: ToolCall[];
    }> {
        /**
         * Integrate with vtcode CLI using "ask" command
         * This spawns a vtcode process and captures the response
         */
        try {
            const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
            if (!workspaceFolder) {
                throw new Error("No workspace folder open");
            }

            // Build conversation context from recent messages
            const recentMessages = this.transcript.slice(-10);
            const _context = recentMessages
                .map((msg) => `[${msg.role}]: ${msg.content}`)
                .join("\n\n");

            // Create terminal for vtcode execution
            const terminalId = `vtcode_ask_${Date.now()}`;
            this.terminalManager.createOrShowTerminal({
                id: terminalId,
                title: "VTCode Agent",
                commandPath: "vtcode",
                args: ["ask", input],
                cwd: workspaceFolder.uri.fsPath,
                env: process.env as NodeJS.ProcessEnv,
            });

            // Buffer output until the CLI completes so we capture the full message.
            return new Promise((resolve, reject) => {
                let buffer = "";
                const timeout = setTimeout(() => {
                    cleanup();
                    reject(new Error("VTCode CLI timeout"));
                }, 30000);

                const cleanup = () => {
                    clearTimeout(timeout);
                    outputListener.dispose();
                    exitListener.dispose();
                };

                const parseResponse = (raw: string) => {
                    const trimmed = raw.trim();
                    if (!trimmed) {
                        return {
                            content: "",
                            metadata: {
                                reasoning: "VTCode CLI produced no output",
                            },
                        };
                    }

                    const lines = trimmed
                        .split(/\r?\n/)
                        .map((line) => line.trim())
                        .filter((line) => line.length > 0);

                    for (let i = lines.length - 1; i >= 0; i -= 1) {
                        const candidate = lines.slice(i).join("");
                        try {
                            const parsed = JSON.parse(candidate);
                            return {
                                content: parsed.content ?? candidate,
                                metadata: parsed.metadata,
                                toolCalls: parsed.toolCalls,
                            };
                        } catch {
                            // Continue searching for a valid JSON payload.
                        }
                    }

                    return {
                        content: trimmed,
                        metadata: {
                            reasoning: "Response from vtcode CLI",
                        },
                    };
                };

                const outputListener = this.terminalManager.onDidReceiveOutput(
                    (event) => {
                        if (event.terminalId === terminalId) {
                            buffer += event.data;
                        }
                    }
                );

                const exitListener = this.terminalManager.onDidExit((event) => {
                    if (event.terminalId !== terminalId) {
                        return;
                    }

                    cleanup();

                    if (event.errorMessage) {
                        reject(new Error(event.errorMessage));
                        return;
                    }

                    try {
                        resolve(parseResponse(buffer));
                    } catch (parseError) {
                        reject(
                            parseError instanceof Error
                                ? parseError
                                : new Error(String(parseError))
                        );
                    }
                });
            });
        } catch (error) {
            // Fallback for development/testing
            return {
                content: `Error calling vtcode CLI: ${
                    error instanceof Error ? error.message : String(error)
                }\n\nFallback response for development.`,
                metadata: {
                    reasoning: "CLI integration error - using fallback",
                },
            };
        }
    }

    private async handleToolCalls(toolCalls: ToolCall[]): Promise<void> {
        for (const toolCall of toolCalls) {
            // Request human approval for tool execution
            const approved = await this.requestToolApproval(toolCall);

            if (!approved) {
                this.sendSystemMessage(
                    `Tool execution cancelled: ${toolCall.name}`,
                    "warning"
                );
                continue;
            }

            // Execute the tool
            const result = await this.executeTool(toolCall);

            // Add tool result to transcript
            const message: ChatMessage = {
                role: "tool",
                content: JSON.stringify(result.result),
                timestamp: Date.now(),
                metadata: {
                    toolResult: result,
                },
            };

            this.addToTranscript(message);
        }
    }

    private async requestToolApproval(toolCall: ToolCall): Promise<boolean> {
        return new Promise((resolve) => {
            const toolId = toolCall.id;
            this.pendingApprovals.set(toolId, resolve);

            // Send approval request to webview
            this.view?.webview.postMessage({
                type: "requestToolApproval",
                toolCall,
            });

            // Auto-reject after 60 seconds
            setTimeout(() => {
                if (this.pendingApprovals.has(toolId)) {
                    this.pendingApprovals.delete(toolId);
                    resolve(false);
                }
            }, 60000);
        });
    }

    private handleToolApproval(toolId: string, approved: boolean): void {
        const resolve = this.pendingApprovals.get(toolId);
        if (resolve) {
            this.pendingApprovals.delete(toolId);
            resolve(approved);
        }
    }

    private async executeTool(toolCall: ToolCall): Promise<ToolResult> {
        const startTime = Date.now();

        try {
            /**
             * Execute tool with proper error handling and logging
             * Supports both native tools and external tool execution
             */
            this.sendSystemMessage(`⚙️ Executing tool: ${toolCall.name}...`);

            const result = await this.invokeToolImplementation(
                toolCall.name,
                toolCall.arguments
            );

            const executionTime = Date.now() - startTime;
            this.sendSystemMessage(
                `✅ Tool ${toolCall.name} completed in ${executionTime}ms`
            );

            return {
                id: toolCall.id,
                name: toolCall.name,
                result,
                executionTimeMs: executionTime,
            };
        } catch (error) {
            const executionTime = Date.now() - startTime;
            const errorMsg =
                error instanceof Error ? error.message : String(error);

            this.sendSystemMessage(
                `❌ Tool ${toolCall.name} failed: ${errorMsg}`,
                "error"
            );

            return {
                id: toolCall.id,
                name: toolCall.name,
                result: null,
                error: errorMsg,
                executionTimeMs: executionTime,
            };
        }
    }

    protected async invokeToolImplementation(
        toolName: string,
        args: Record<string, unknown>
    ): Promise<unknown> {
        /**
         * Comprehensive tool invocation with full native tool support
         * Implements all VTCode native tools using VSCode APIs
         */
        switch (toolName) {
            case "run_command":
            case "run_terminal_cmd":
                return await this.runCommandInPty(args.command as string);

            case "read_file":
                return await this.readFile(
                    args.path as string,
                    args.start_line as number | undefined,
                    args.limit as number | undefined
                );

            case "write_file":
                return await this.writeFile(
                    args.path as string,
                    args.content as string,
                    (args.mode as string) || "overwrite"
                );

            case "edit_file":
                return await this.editFile(
                    args.path as string,
                    args.old_str as string,
                    args.new_str as string
                );

            case "list_files":
                return await this.listFiles(
                    (args.path as string) || ".",
                    (args.mode as string) || "tree"
                );

            case "grep_file":
                return await this.grepFile(
                    args.pattern as string,
                    (args.path as string) || "**/*"
                );

            case "delete_file":
                return await this.deleteFile(args.path as string);

            case "create_pty_session":
                return await this.createPtySession(
                    (args.session_id as string) || `pty_${Date.now()}`,
                    (args.cwd as string) || "."
                );

            case "send_pty_input":
                return await this.sendPtyInput(
                    args.session_id as string,
                    args.input as string
                );

            case "read_pty_session":
                return await this.readPtySession(args.session_id as string);

            default:
                throw new Error(`Unknown tool: ${toolName}`);
        }
    }

    /**
     * Enhanced read_file with line-based reading support
     */
    private async readFile(
        filePath: string,
        startLine?: number,
        limit?: number
    ): Promise<unknown> {
        const uri = this.resolveWorkspacePath(filePath);
        const content = await vscode.workspace.fs.readFile(uri);
        const text = Buffer.from(content).toString("utf8");

        if (startLine !== undefined) {
            const lines = text.split("\n");
            const end = limit ? startLine + limit : lines.length;
            const selectedLines = lines.slice(startLine - 1, end);

            return {
                success: true,
                path: filePath,
                content: selectedLines.join("\n"),
                lines: {
                    start: startLine,
                    end: Math.min(end, lines.length),
                    total: lines.length,
                },
            };
        }

        return {
            success: true,
            path: filePath,
            content: text,
            size: content.byteLength,
        };
    }

    /**
     * Enhanced write_file with mode support
     */
    private async writeFile(
        filePath: string,
        content: string,
        mode: string
    ): Promise<unknown> {
        const uri = this.resolveWorkspacePath(filePath);

        // Check if file exists
        let fileExists = false;
        try {
            await vscode.workspace.fs.stat(uri);
            fileExists = true;
        } catch {
            // File doesn't exist
        }

        // Handle modes
        if (mode === "skip_if_exists" && fileExists) {
            return {
                success: true,
                path: filePath,
                skipped: true,
                reason: "File already exists",
            };
        }

        let finalContent = content;
        if (mode === "append" && fileExists) {
            const existing = await vscode.workspace.fs.readFile(uri);
            finalContent =
                Buffer.from(existing).toString("utf8") + "\n" + content;
        }

        await vscode.workspace.fs.writeFile(
            uri,
            Buffer.from(finalContent, "utf8")
        );

        return {
            success: true,
            path: filePath,
            mode,
            size: Buffer.byteLength(finalContent, "utf8"),
        };
    }

    /**
     * Edit file by replacing text
     */
    private async editFile(
        filePath: string,
        oldStr: string,
        newStr: string
    ): Promise<unknown> {
        const uri = this.resolveWorkspacePath(filePath);
        const content = await vscode.workspace.fs.readFile(uri);
        const text = Buffer.from(content).toString("utf8");

        const occurrences = (
            text.match(new RegExp(this.escapeRegex(oldStr), "g")) || []
        ).length;

        if (occurrences === 0) {
            throw new Error(`Old string not found in ${filePath}`);
        }

        if (occurrences > 1) {
            throw new Error(
                `Old string found ${occurrences} times in ${filePath}. Please be more specific.`
            );
        }

        const newText = text.replace(oldStr, newStr);
        await vscode.workspace.fs.writeFile(uri, Buffer.from(newText, "utf8"));

        return {
            success: true,
            path: filePath,
            replaced: 1,
            size: Buffer.byteLength(newText, "utf8"),
        };
    }

    /**
     * List files in directory
     */
    private async listFiles(path: string, mode: string): Promise<unknown> {
        const uri = this.resolveWorkspacePath(path);
        const entries = await vscode.workspace.fs.readDirectory(uri);
        const files: Array<{ name: string; type: string }> = [];

        for (const [name, type] of entries) {
            if (
                name.startsWith(".") ||
                name === "node_modules" ||
                name === "target"
            ) {
                continue;
            }

            files.push({
                name,
                type: type === vscode.FileType.Directory ? "directory" : "file",
            });
        }

        return {
            success: true,
            path,
            mode,
            files,
            count: files.length,
        };
    }

    /**
     * Search files with pattern
     */
    private async grepFile(pattern: string, _path: string): Promise<unknown> {
        const results = await vscode.workspace.findFiles(
            "**/*",
            "**/node_modules/**",
            100
        );
        const matches: Array<{
            file: string;
            line: number;
            text: string;
        }> = [];

        // Search through files for pattern
        const regex = new RegExp(pattern, "gi");

        for (const uri of results) {
            try {
                const content = await vscode.workspace.fs.readFile(uri);
                const text = Buffer.from(content).toString("utf8");
                const lines = text.split("\n");

                for (let i = 0; i < lines.length; i++) {
                    if (regex.test(lines[i])) {
                        matches.push({
                            file: uri.fsPath,
                            line: i + 1,
                            text: lines[i].trim(),
                        });
                    }
                }
            } catch {
                // Skip files that can't be read
            }
        }

        return {
            success: true,
            pattern,
            matches: matches.slice(0, 100), // Limit results
            count: matches.length,
        };
    }

    /**
     * Delete file
     */
    private async deleteFile(filePath: string): Promise<unknown> {
        const uri = this.resolveWorkspacePath(filePath);
        await vscode.workspace.fs.delete(uri);

        return {
            success: true,
            path: filePath,
            deleted: true,
        };
    }

    /**
     * Create PTY session
     */
    private async createPtySession(
        sessionId: string,
        cwd: string
    ): Promise<unknown> {
        const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
        if (!workspaceFolder) {
            throw new Error("No workspace folder open");
        }

        const workingDir = cwd === "." ? workspaceFolder.uri.fsPath : cwd;

        const terminal = this.terminalManager.createOrShowTerminal({
            id: sessionId,
            title: `PTY: ${sessionId}`,
            commandPath: process.env.SHELL || "/bin/bash",
            args: [],
            cwd: workingDir,
            env: process.env as NodeJS.ProcessEnv,
        });

        return {
            success: true,
            session_id: sessionId,
            created: terminal.created,
        };
    }

    /**
     * Send input to PTY session
     */
    private async sendPtyInput(
        sessionId: string,
        input: string
    ): Promise<unknown> {
        const sent = this.terminalManager.sendText(sessionId, input);

        if (!sent) {
            throw new Error(`PTY session ${sessionId} not found`);
        }

        return {
            success: true,
            session_id: sessionId,
            sent: true,
        };
    }

    /**
     * Read PTY session status
     */
    private async readPtySession(sessionId: string): Promise<unknown> {
        const handle = this.terminalManager.getTerminalHandle(sessionId);

        if (!handle) {
            throw new Error(`PTY session ${sessionId} not found`);
        }

        return {
            success: true,
            session_id: sessionId,
            running: handle.isRunning(),
        };
    }

    /**
     * Resolve workspace-relative path
     */
    private resolveWorkspacePath(path: string): vscode.Uri {
        if (path.startsWith("/")) {
            return vscode.Uri.file(path);
        }

        const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
        if (!workspaceFolder) {
            throw new Error("No workspace folder open");
        }

        return vscode.Uri.joinPath(workspaceFolder.uri, path);
    }

    /**
     * Escape string for regex
     */
    private escapeRegex(str: string): string {
        return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    }

    protected async invokeTool(
        toolName: string,
        args: Record<string, string>
    ): Promise<void> {
        const toolCall: ToolCall = {
            id: `tool_${this.messageIdCounter++}`,
            name: toolName,
            arguments: args,
        };

        const result = await this.executeTool(toolCall);

        const message: ChatMessage = {
            role: "tool",
            content: JSON.stringify(result.result),
            timestamp: Date.now(),
            metadata: {
                toolResult: result,
            },
        };

        this.addToTranscript(message);
    }

    private async runCommandInPty(command: string): Promise<unknown> {
        /**
         * Execute command in PTY using VtcodeTerminalManager
         * Creates a terminal session, runs the command, and captures output
         */
        const workspaceFolders = vscode.workspace.workspaceFolders;
        if (!workspaceFolders || workspaceFolders.length === 0) {
            throw new Error("No workspace folder open");
        }

        const sessionId = `cmd_${Date.now()}`;

        try {
            // Create PTY session
            this.terminalManager.createOrShowTerminal({
                id: sessionId,
                title: "Command Execution",
                commandPath: process.env.SHELL || "/bin/bash",
                args: ["-c", command],
                cwd: workspaceFolders[0].uri.fsPath,
                env: process.env as NodeJS.ProcessEnv,
            });

            // Capture output
            return new Promise((resolve, reject) => {
                const timeout = setTimeout(() => {
                    reject(new Error("Command execution timeout"));
                }, 30000);

                let output = "";

                const outputListener = this.terminalManager.onDidReceiveOutput(
                    (event) => {
                        if (event.terminalId === sessionId) {
                            output += event.data;
                        }
                    }
                );

                const exitListener = this.terminalManager.onDidExit((event) => {
                    if (event.terminalId === sessionId) {
                        clearTimeout(timeout);
                        outputListener.dispose();
                        exitListener.dispose();

                        if (event.errorMessage) {
                            reject(new Error(event.errorMessage));
                        } else {
                            resolve({
                                success: true,
                                command,
                                stdout: output,
                                exit_code: event.code || 0,
                                mode: "pty",
                            });
                        }
                    }
                });
            });
        } catch (error) {
            throw new Error(
                `Command execution failed: ${
                    error instanceof Error ? error.message : String(error)
                }`
            );
        }
    }

    private async analyzeCode(): Promise<void> {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            this.sendSystemMessage("No active editor", "warning");
            return;
        }

        const selection = editor.selection;
        const text = editor.document.getText(
            selection.isEmpty ? undefined : selection
        );

        await this.processAgentResponse(`Analyze this code:\n\n${text}`);
    }

    private async explainSelection(): Promise<void> {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            this.sendSystemMessage("No active editor", "warning");
            return;
        }

        const selection = editor.selection;
        if (selection.isEmpty) {
            this.sendSystemMessage("No text selected", "warning");
            return;
        }

        const text = editor.document.getText(selection);
        await this.processAgentResponse(`Explain this code:\n\n${text}`);
    }

    private async refactorCode(): Promise<void> {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            this.sendSystemMessage("No active editor", "warning");
            return;
        }

        const selection = editor.selection;
        const text = editor.document.getText(
            selection.isEmpty ? undefined : selection
        );

        await this.processAgentResponse(
            `Suggest refactorings for this code:\n\n${text}`
        );
    }

    private async generateTests(): Promise<void> {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            this.sendSystemMessage("No active editor", "warning");
            return;
        }

        const selection = editor.selection;
        const text = editor.document.getText(
            selection.isEmpty ? undefined : selection
        );

        await this.processAgentResponse(
            `Generate unit tests for this code:\n\n${text}`
        );
    }

    private async handleCommandExecution(command: string): Promise<void> {
        this.sendSystemMessage(`Executing: ${command}`);
        try {
            const result = await this.runCommandInPty(command);
            this.sendSystemMessage(`Result: ${result}`);
        } catch (error) {
            this.sendSystemMessage(
                `Error: ${
                    error instanceof Error ? error.message : String(error)
                }`,
                "error"
            );
        }
    }

    private currentOperationAbortController: AbortController | null = null;

    private handleCancelOperation(): void {
        /**
         * Cancel current operation by aborting active requests
         * and clearing pending approvals
         */
        // Abort any ongoing operations
        if (this.currentOperationAbortController) {
            this.currentOperationAbortController.abort();
            this.currentOperationAbortController = null;
            this.sendSystemMessage("⚠️ Operation cancelled by user", "warning");
        }

        // Clear pending tool approvals
        if (this.pendingApprovals.size > 0) {
            const count = this.pendingApprovals.size;
            for (const [_toolId, resolve] of this.pendingApprovals) {
                resolve(false);
            }
            this.pendingApprovals.clear();
            this.sendSystemMessage(
                `⚠️ Cancelled ${count} pending tool approval(s)`,
                "warning"
            );
        }

        // Hide thinking indicator
        this.sendThinkingIndicator(false);

        this.sendSystemMessage("✅ All operations cancelled");
    }

    private clearTranscript(): void {
        this.transcript = [];
        this.view?.webview.postMessage({
            type: "clearTranscript",
        });
    }

    private async exportTranscript(): Promise<void> {
        const content = JSON.stringify(this.transcript, null, 2);
        const uri = await vscode.window.showSaveDialog({
            filters: { JSON: ["json"] },
            defaultUri: vscode.Uri.file("vtcode-transcript.json"),
        });

        if (uri) {
            await vscode.workspace.fs.writeFile(
                uri,
                Buffer.from(content, "utf8")
            );
            this.sendSystemMessage(`Transcript exported to ${uri.fsPath}`);
        }
    }

    private showStats(): void {
        const userMessages = this.transcript.filter(
            (m) => m.role === "user"
        ).length;
        const assistantMessages = this.transcript.filter(
            (m) => m.role === "assistant"
        ).length;
        const toolCalls = this.transcript.filter(
            (m) => m.role === "tool"
        ).length;

        const stats = `
📊 Session Statistics:
- User messages: ${userMessages}
- Assistant messages: ${assistantMessages}
- Tool calls: ${toolCalls}
- Total entries: ${this.transcript.length}
		`.trim();

        this.sendSystemMessage(stats);
    }

    private async showConfig(): Promise<void> {
        // Load and display vtcode.toml config
        try {
            const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
            if (!workspaceFolder) {
                this.sendSystemMessage("No workspace folder open", "warning");
                return;
            }

            const configUri = vscode.Uri.joinPath(
                workspaceFolder.uri,
                "vtcode.toml"
            );

            try {
                const content = await vscode.workspace.fs.readFile(configUri);
                const configText = Buffer.from(content).toString("utf8");

                this.sendSystemMessage(
                    `⚙️ Current vtcode.toml configuration:\n\n\`\`\`toml\n${configText}\n\`\`\``
                );
            } catch {
                this.sendSystemMessage(
                    "vtcode.toml not found in workspace",
                    "warning"
                );
            }
        } catch (error) {
            this.sendSystemMessage(
                `Failed to load configuration: ${
                    error instanceof Error ? error.message : String(error)
                }`,
                "error"
            );
        }
    }

    private getHelpText(): string {
        return `
🤖 VTCode Chat Commands:

System Commands (prefix: /):
  /clear     - Clear conversation transcript
  /help      - Show this help message
  /export    - Export transcript to file
  /stats     - Show session statistics
  /config    - Show current configuration

Agent Commands (prefix: @):
  @analyze   - Analyze selected code
  @explain   - Explain selected code
  @refactor  - Suggest refactorings
  @test      - Generate unit tests

Tool Commands (prefix: #):
  #run command="..." - Execute shell command
  #read path="..."   - Read file contents
  #write path="..." content="..." - Write to file

Just type your message to chat with the agent!
		`.trim();
    }

    protected sendSystemMessage(
        content: string,
        level: "info" | "warning" | "error" = "info"
    ): void {
        const message: ChatMessage = {
            role: "system",
            content,
            timestamp: Date.now(),
        };

        this.addToTranscript(message);

        // Also show in VS Code notification for errors
        if (level === "error") {
            vscode.window.showErrorMessage(content);
        } else if (level === "warning") {
            vscode.window.showWarningMessage(content);
        }
    }

    private sendThinkingIndicator(thinking: boolean): void {
        this.view?.webview.postMessage({
            type: "thinking",
            thinking,
        });
    }

    /**
     * Get the current transcript (for subclasses)
     */
    protected getTranscript(): TranscriptEntry[] {
        return this.transcript;
    }

    /**
     * Add message to transcript (public for subclasses)
     */
    protected addToTranscript(message: ChatMessage): void {
        const entry: TranscriptEntry = {
            ...message,
            id: `msg_${this.messageIdCounter++}`,
        };

        this.transcript.push(entry);

        this.view?.webview.postMessage({
            type: "addMessage",
            message: entry,
        });
    }

    private getHtmlForWebview(webview: vscode.Webview): string {
        const styleUri = webview.asWebviewUri(
            vscode.Uri.joinPath(
                this.context.extensionUri,
                "media",
                "chat-view.css"
            )
        );
        const scriptUri = webview.asWebviewUri(
            vscode.Uri.joinPath(
                this.context.extensionUri,
                "media",
                "chat-view.js"
            )
        );

        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src ${webview.cspSource};">
    <link href="${styleUri}" rel="stylesheet">
    <title>VTCode Chat</title>
</head>
<body>
    <div id="chat-container">
        <div id="transcript-container"></div>
        <div id="thinking-indicator" style="display: none;">
            <span class="thinking-dots">⚫⚫⚫</span> Agent is thinking...
        </div>
        <div id="approval-panel" style="display: none;"></div>
        <div id="input-container">
            <textarea id="user-input" placeholder="Type your message... (Use /, @, or # for commands)" rows="3"></textarea>
            <div id="button-container">
                <button id="send-button">Send</button>
                <button id="clear-button">Clear</button>
                <button id="cancel-button">Cancel</button>
            </div>
        </div>
    </div>
    <script src="${scriptUri}"></script>
</body>
</html>`;
    }
}
