/**
 * VTCode Backend Integration
 *
 * Handles communication between VS Code extension and vtcode CLI
 */

import { spawn } from "node:child_process";
import * as vscode from "vscode";

export interface VtcodeRequest {
    prompt: string;
    conversationHistory?: ConversationMessage[];
    tools?: ToolDefinition[];
    config?: VtcodeConfig;
}

export interface ConversationMessage {
    role: "user" | "assistant" | "system" | "tool";
    content: string;
    metadata?: Record<string, unknown>;
}

export interface ToolDefinition {
    name: string;
    description: string;
    parameters: Record<string, unknown>;
}

export interface VtcodeConfig {
    model?: string;
    maxTokens?: number;
    temperature?: number;
    reasoningEffort?: string;
}

export interface VtcodeResponse {
    content: string;
    reasoning?: string;
    toolCalls?: ToolCallResponse[];
    finishReason?: string;
    usage?: {
        promptTokens: number;
        completionTokens: number;
        totalTokens: number;
    };
}

export interface ToolCallResponse {
    id: string;
    name: string;
    arguments: Record<string, unknown>;
}

export interface VtcodeStreamChunk {
    type: "content" | "reasoning" | "toolCall" | "done" | "error";
    data: unknown;
}

export class VtcodeBackend {
    private vtcodePath: string;
    private workspaceRoot?: string;
    private outputChannel: vscode.OutputChannel;

    constructor(
        vtcodePath: string,
        workspaceRoot: string | undefined,
        outputChannel: vscode.OutputChannel
    ) {
        this.vtcodePath = vtcodePath;
        this.workspaceRoot = workspaceRoot;
        this.outputChannel = outputChannel;
    }

    /**
     * Execute a single prompt using vtcode CLI
     */
    async executePrompt(request: VtcodeRequest): Promise<VtcodeResponse> {
        return new Promise((resolve, reject) => {
            const args = ["ask", request.prompt];

            // Add configuration flags if provided
            if (request.config?.model) {
                args.push("--model", request.config.model);
            }
            if (request.config?.maxTokens) {
                args.push("--max-tokens", String(request.config.maxTokens));
            }

            this.outputChannel.appendLine(
                `Executing: ${this.vtcodePath} ${args.join(" ")}`
            );

            const childProcess = spawn(this.vtcodePath, args, {
                cwd: this.workspaceRoot,
                env: {
                    ...process.env,
                    RUST_LOG: "info",
                },
            });

            let stdout = "";
            let stderr = "";

            childProcess.stdout.on("data", (data: Buffer) => {
                stdout += data.toString();
            });

            childProcess.stderr.on("data", (data: Buffer) => {
                stderr += data.toString();
                this.outputChannel.appendLine(`[stderr] ${data.toString()}`);
            });

            childProcess.on("close", (code: number | null) => {
                if (code !== 0) {
                    reject(
                        new Error(`vtcode exited with code ${code}: ${stderr}`)
                    );
                    return;
                }

                try {
                    // Parse the output
                    const response = this.parseVtcodeOutput(stdout);
                    resolve(response);
                } catch (error) {
                    reject(error);
                }
            });

            childProcess.on("error", (error: Error) => {
                reject(error);
            });
        });
    }

    /**
     * Stream a response from vtcode CLI
     */
    async *streamPrompt(
        request: VtcodeRequest,
        cancellationToken?: vscode.CancellationToken
    ): AsyncGenerator<VtcodeStreamChunk> {
        const args = ["ask", request.prompt, "--stream"];

        if (request.config?.model) {
            args.push("--model", request.config.model);
        }

        this.outputChannel.appendLine(
            `Streaming: ${this.vtcodePath} ${args.join(" ")}`
        );

        const childProcess = spawn(this.vtcodePath, args, {
            cwd: this.workspaceRoot,
            env: {
                ...process.env,
                RUST_LOG: "info",
            },
        });

        // Handle cancellation
        if (cancellationToken) {
            cancellationToken.onCancellationRequested(() => {
                childProcess.kill("SIGTERM");
            });
        }

        const lineBuffer: string[] = [];

        childProcess.stdout.on("data", (data: Buffer) => {
            const lines = data.toString().split("\n");
            for (const line of lines) {
                if (line.trim()) {
                    lineBuffer.push(line);
                }
            }
        });

        // Yield chunks as they arrive
        while (!childProcess.killed) {
            if (lineBuffer.length > 0) {
                const line = lineBuffer.shift()!;
                try {
                    const chunk = JSON.parse(line) as VtcodeStreamChunk;
                    yield chunk;
                } catch {
                    // Not JSON, treat as plain text
                    yield { type: "content", data: line };
                }
            }
            await new Promise((resolve) => setTimeout(resolve, 10));
        }

        // Wait for process to exit
        await new Promise<void>((resolve, reject) => {
            childProcess.on("close", (code: number | null) => {
                if (code === 0 || code === null) {
                    resolve();
                } else {
                    reject(new Error(`Process exited with code ${code}`));
                }
            });
            childProcess.on("error", reject);
        });
    }

    /**
     * Execute a tool through vtcode
     */
    async executeTool(
        toolName: string,
        args: Record<string, unknown>
    ): Promise<unknown> {
        const argsJson = JSON.stringify(args);
        const process = spawn(
            this.vtcodePath,
            ["tool", "execute", toolName, "--args", argsJson],
            {
                cwd: this.workspaceRoot,
            }
        );

        return new Promise((resolve, reject) => {
            let stdout = "";
            let stderr = "";

            process.stdout.on("data", (data) => {
                stdout += data.toString();
            });

            process.stderr.on("data", (data) => {
                stderr += data.toString();
            });

            process.on("close", (code) => {
                if (code !== 0) {
                    reject(new Error(`Tool execution failed: ${stderr}`));
                    return;
                }

                try {
                    resolve(JSON.parse(stdout));
                } catch {
                    resolve(stdout);
                }
            });

            process.on("error", reject);
        });
    }

    /**
     * Get available tools from vtcode
     */
    async getAvailableTools(): Promise<ToolDefinition[]> {
        return new Promise((resolve, reject) => {
            const process = spawn(this.vtcodePath, ["tool", "list", "--json"], {
                cwd: this.workspaceRoot,
            });

            let stdout = "";

            process.stdout.on("data", (data) => {
                stdout += data.toString();
            });

            process.on("close", (code) => {
                if (code !== 0) {
                    reject(new Error("Failed to get tools list"));
                    return;
                }

                try {
                    const tools = JSON.parse(stdout) as ToolDefinition[];
                    resolve(tools);
                } catch {
                    resolve([]);
                }
            });

            process.on("error", reject);
        });
    }

    /**
     * Parse vtcode output into structured response
     */
    private parseVtcodeOutput(output: string): VtcodeResponse {
        // Try to parse as JSON first
        try {
            const parsed = JSON.parse(output) as VtcodeResponse;
            return parsed;
        } catch {
            // Plain text response
            return {
                content: output.trim(),
                finishReason: "stop",
            };
        }
    }

    /**
     * Check if vtcode is available
     */
    static async isAvailable(vtcodePath: string): Promise<boolean> {
        return new Promise((resolve) => {
            const process = spawn(vtcodePath, ["--version"]);

            process.on("close", (code) => {
                resolve(code === 0);
            });

            process.on("error", () => {
                resolve(false);
            });

            // Timeout after 5 seconds
            setTimeout(() => {
                process.kill();
                resolve(false);
            }, 5000);
        });
    }
}

/**
 * Create a VtcodeBackend instance with automatic path detection
 */
export async function createVtcodeBackend(
    outputChannel: vscode.OutputChannel
): Promise<VtcodeBackend | null> {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    const workspaceRoot = workspaceFolders?.[0]?.uri.fsPath;

    // Try to find vtcode in PATH or configured location
    const config = vscode.workspace.getConfiguration("vtcode");
    const configuredPath = config.get<string>("cli.path");

    let vtcodePath = configuredPath || "vtcode";

    // Check if vtcode is available
    const available = await VtcodeBackend.isAvailable(vtcodePath);
    if (!available) {
        outputChannel.appendLine(`vtcode CLI not found at: ${vtcodePath}`);

        // Try alternative paths
        const alternatives = ["./vtcode", "../vtcode", "cargo run --"];
        for (const alt of alternatives) {
            if (await VtcodeBackend.isAvailable(alt)) {
                vtcodePath = alt;
                outputChannel.appendLine(`Found vtcode at: ${vtcodePath}`);
                break;
            }
        }

        if (!available) {
            return null;
        }
    }

    return new VtcodeBackend(vtcodePath, workspaceRoot, outputChannel);
}
