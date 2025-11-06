/**
 * VTCode Backend Integration Layer
 *
 * Provides integration between the chat UI and VTCode CLI backend
 * using a clean composition pattern.
 */

import * as vscode from "vscode";
import type { Context7Integration } from "./context7Integration";
import type { ChatMessage } from "./enhancedChatView";
import type { McpToolManager } from "./mcpTools";

export interface VTCodeBackendConfig {
    vtcodePath: string;
    workspaceRoot: string;
    timeout: number;
}

export interface AgentResponse {
    content: string;
    metadata?: {
        model?: string;
        tokens?: { prompt: number; completion: number; total: number };
        reasoning?: string;
    };
}

/**
 * VTCode Backend Integration Service
 */
export class VTCodeBackend {
    private config: VTCodeBackendConfig;
    private outputChannel: vscode.OutputChannel;
    private context7: Context7Integration | null;
    private mcpManager: McpToolManager | null;

    constructor(
        outputChannel: vscode.OutputChannel,
        context7: Context7Integration | null,
        mcpManager: McpToolManager | null,
        config?: Partial<VTCodeBackendConfig>
    ) {
        this.outputChannel = outputChannel;
        this.context7 = context7;
        this.mcpManager = mcpManager;
        this.config = {
            vtcodePath: config?.vtcodePath || "vtcode",
            workspaceRoot:
                config?.workspaceRoot ||
                vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ||
                "",
            timeout: config?.timeout || 60000,
        };

        this.outputChannel.appendLine("[VTCodeBackend] Initialized");
    }

    /**
     * Process user query with full context
     */
    async processQuery(
        query: string,
        conversationHistory: ChatMessage[]
    ): Promise<AgentResponse> {
        try {
            // Step 1: Enhance with Context7 if available
            let enhancedQuery = query;
            if (this.context7) {
                try {
                    const workspaceContext = await this.getWorkspaceContext();
                    enhancedQuery = await this.context7.enhanceQuery(
                        query,
                        workspaceContext
                    );
                    this.outputChannel.appendLine(
                        "[VTCodeBackend] Query enhanced with Context7"
                    );
                } catch (error) {
                    this.outputChannel.appendLine(
                        `[VTCodeBackend] Context7 enhancement failed: ${error}`
                    );
                }
            }

            // Step 2: Build conversation context
            const context = this.buildConversationContext(conversationHistory);

            // Step 3: Call VTCode CLI
            const response = await this.callVTCodeCLI(enhancedQuery, context);

            return response;
        } catch (error) {
            this.outputChannel.appendLine(
                `[VTCodeBackend] Error: ${
                    error instanceof Error ? error.message : String(error)
                }`
            );

            // Return fallback response
            return {
                content: `I encountered an error processing your request: ${
                    error instanceof Error ? error.message : String(error)
                }\n\nPlease try again or check the output channel for details.`,
                metadata: {
                    model: "error-fallback",
                },
            };
        }
    }

    /**
     * Call VTCode CLI with full context integration
     */
    private async callVTCodeCLI(
        query: string,
        context: string
    ): Promise<AgentResponse> {
        /**
         * Execute vtcode CLI with "ask" command
         * Integrates conversation history and workspace context
         */
        try {
            const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
            if (!workspaceFolder) {
                throw new Error("No workspace folder open");
            }

            this.outputChannel.appendLine(
                `[Backend] Executing vtcode CLI: ask "${query.slice(0, 50)}..."`
            );

            // Build command with context
            const { exec } = await import("child_process");
            const { promisify } = await import("util");
            const execAsync = promisify(exec);

            const args = ["ask", query];
            if (context && context.trim()) {
                args.push("--context", context.slice(0, 2000));
            }

            const commandStr = `${this.config.vtcodePath} ${args
                .map((arg) => `"${arg.replace(/"/g, '\\"')}"`)
                .join(" ")}`;

            this.outputChannel.appendLine(`[Backend] Command: ${commandStr}`);

            const { stdout, stderr } = await execAsync(commandStr, {
                cwd: workspaceFolder.uri.fsPath,
                timeout: this.config.timeout,
                maxBuffer: 10 * 1024 * 1024,
                env: process.env,
            });

            if (stderr) {
                this.outputChannel.appendLine(`[Backend] stderr: ${stderr}`);
            }

            this.outputChannel.appendLine(
                `[Backend] Received response: ${stdout.slice(0, 100)}...`
            );

            // Parse response
            try {
                const parsed = JSON.parse(stdout);
                return {
                    content: parsed.content || parsed.response || stdout,
                    metadata: {
                        model: parsed.model || "vtcode-cli",
                        tokens: parsed.tokens || {
                            prompt: 0,
                            completion: 0,
                            total: 0,
                        },
                        reasoning: parsed.reasoning,
                    },
                };
            } catch {
                // Plain text response
                return {
                    content: stdout,
                    metadata: {
                        model: "vtcode-cli",
                        tokens: { prompt: 0, completion: 0, total: 0 },
                    },
                };
            }
        } catch (error) {
            this.outputChannel.appendLine(
                `[Backend] CLI error: ${
                    error instanceof Error ? error.message : String(error)
                }`
            );

            throw new Error(
                `VTCode CLI execution failed: ${
                    error instanceof Error ? error.message : String(error)
                }`
            );
        }
    }

    /**
     * Build conversation context
     */
    private buildConversationContext(history: ChatMessage[]): string {
        const recentMessages = history.slice(-10);
        return recentMessages
            .map((msg: ChatMessage) => `[${msg.role}]: ${msg.content}`)
            .join("\n\n");
    }

    /**
     * Get current workspace context
     */
    private async getWorkspaceContext(): Promise<string> {
        const context: string[] = [];

        // Active editor
        const activeEditor = vscode.window.activeTextEditor;
        if (activeEditor) {
            const fileName = activeEditor.document.fileName;
            const language = activeEditor.document.languageId;
            context.push(`Active file: ${fileName} (${language})`);

            // Selection or preview
            const selection = activeEditor.selection;
            if (!selection.isEmpty) {
                const selectedText = activeEditor.document.getText(selection);
                context.push(`Selected code:\n${selectedText.slice(0, 500)}`);
            } else {
                const firstLines = activeEditor.document
                    .getText()
                    .slice(0, 500);
                context.push(`File content (preview):\n${firstLines}`);
            }
        }

        // Workspace folder
        const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
        if (workspaceFolder) {
            context.push(`Workspace: ${workspaceFolder.name}`);
        }

        return context.join("\n\n");
    }

    /**
     * Get Context7 instance
     */
    getContext7(): Context7Integration | null {
        return this.context7;
    }

    /**
     * Get MCP manager
     */
    getMcpManager(): McpToolManager | null {
        return this.mcpManager;
    }
}

/**
 * Factory function to create VTCode backend
 */
export async function createVTCodeBackend(
    outputChannel: vscode.OutputChannel,
    config?: Partial<VTCodeBackendConfig>
): Promise<VTCodeBackend> {
    // Create MCP tool manager
    const { createMcpToolManager } = await import("./mcpTools");
    const mcpManager = await createMcpToolManager(outputChannel);

    // Create Context7 integration
    const { createContext7Integration } = await import("./context7Integration");
    const context7 = await createContext7Integration(mcpManager, outputChannel);

    // Create backend
    const backend = new VTCodeBackend(
        outputChannel,
        context7,
        mcpManager,
        config
    );

    outputChannel.appendLine("[Factory] VTCodeBackend created successfully");

    return backend;
}
