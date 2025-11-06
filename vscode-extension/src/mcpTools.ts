/**
 * MCP Tool Integration for VTCode Chat Extension
 *
 * Provides Model Context Protocol (MCP) tool support for enhanced
 * chat capabilities including external tool invocations.
 */

import { spawn } from "node:child_process";
import * as vscode from "vscode";

export interface McpTool {
    name: string;
    description: string;
    inputSchema: Record<string, unknown>;
    provider: string;
}

export interface McpProvider {
    name: string;
    command: string;
    args: string[];
    enabled: boolean;
    env?: Record<string, string>;
}

export interface McpToolInvocation {
    provider: string;
    tool: string;
    arguments: Record<string, unknown>;
}

export interface McpToolResult {
    success: boolean;
    result?: unknown;
    error?: string;
    executionTimeMs: number;
}

export class McpToolManager {
    private providers: Map<string, McpProvider> = new Map();
    private tools: Map<string, McpTool> = new Map();
    private outputChannel: vscode.OutputChannel;

    constructor(outputChannel: vscode.OutputChannel) {
        this.outputChannel = outputChannel;
    }

    /**
     * Load MCP providers from vtcode.toml configuration
     */
    async loadProviders(workspaceRoot: string): Promise<void> {
        try {
            const configUri = vscode.Uri.file(`${workspaceRoot}/vtcode.toml`);
            const configContent = await vscode.workspace.fs.readFile(configUri);
            const config = this.parseToml(
                Buffer.from(configContent).toString("utf8")
            );

            if (config.mcp?.providers) {
                for (const provider of config.mcp.providers) {
                    if (provider.enabled !== false) {
                        this.providers.set(provider.name, provider);
                        this.outputChannel.appendLine(
                            `[MCP] Loaded provider: ${provider.name}`
                        );
                    }
                }
            }

            // Discover tools from each provider
            await this.discoverTools();
        } catch (error) {
            this.outputChannel.appendLine(
                `[MCP] Failed to load providers: ${
                    error instanceof Error ? error.message : String(error)
                }`
            );
        }
    }

    /**
     * Discover available tools from all enabled providers
     */
    private async discoverTools(): Promise<void> {
        for (const [providerName, provider] of this.providers) {
            try {
                const tools = await this.queryProviderTools(provider);
                for (const tool of tools) {
                    const fullName = `${providerName}/${tool.name}`;
                    this.tools.set(fullName, {
                        ...tool,
                        provider: providerName,
                    });
                    this.outputChannel.appendLine(
                        `[MCP] Discovered tool: ${fullName}`
                    );
                }
            } catch (error) {
                this.outputChannel.appendLine(
                    `[MCP] Failed to discover tools from ${providerName}: ${
                        error instanceof Error ? error.message : String(error)
                    }`
                );
            }
        }
    }

    /**
     * Query a provider for its available tools
     */
    private async queryProviderTools(
        provider: McpProvider
    ): Promise<McpTool[]> {
        return new Promise((resolve, reject) => {
            const proc = spawn(
                provider.command,
                [...provider.args, "--list-tools"],
                {
                    env: { ...process.env, ...provider.env },
                }
            );

            let stdout = "";
            let stderr = "";

            proc.stdout.on("data", (data: Buffer) => {
                stdout += data.toString();
            });

            proc.stderr.on("data", (data: Buffer) => {
                stderr += data.toString();
            });

            proc.on("close", (code: number | null) => {
                if (code !== 0) {
                    reject(
                        new Error(
                            `Provider exited with code ${code}: ${stderr}`
                        )
                    );
                    return;
                }

                try {
                    const tools = JSON.parse(stdout) as McpTool[];
                    resolve(tools);
                } catch (error) {
                    reject(new Error(`Failed to parse tools: ${error}`));
                }
            });

            proc.on("error", (error: Error) => {
                reject(error);
            });

            // Timeout after 5 seconds
            setTimeout(() => {
                proc.kill();
                reject(new Error("Tool discovery timed out"));
            }, 5000);
        });
    }

    /**
     * Invoke an MCP tool
     */
    async invokeTool(invocation: McpToolInvocation): Promise<McpToolResult> {
        const startTime = Date.now();

        const toolName = invocation.tool.includes("/")
            ? invocation.tool
            : `${invocation.provider}/${invocation.tool}`;

        const tool = this.tools.get(toolName);
        if (!tool) {
            return {
                success: false,
                error: `Tool not found: ${toolName}`,
                executionTimeMs: Date.now() - startTime,
            };
        }

        const provider = this.providers.get(tool.provider);
        if (!provider) {
            return {
                success: false,
                error: `Provider not found: ${tool.provider}`,
                executionTimeMs: Date.now() - startTime,
            };
        }

        try {
            const result = await this.executeToolViaProvider(
                provider,
                tool,
                invocation.arguments
            );

            return {
                success: true,
                result,
                executionTimeMs: Date.now() - startTime,
            };
        } catch (error) {
            return {
                success: false,
                error: error instanceof Error ? error.message : String(error),
                executionTimeMs: Date.now() - startTime,
            };
        }
    }

    /**
     * Execute a tool via its provider
     */
    private async executeToolViaProvider(
        provider: McpProvider,
        tool: McpTool,
        args: Record<string, unknown>
    ): Promise<unknown> {
        return new Promise((resolve, reject) => {
            const proc = spawn(
                provider.command,
                [
                    ...provider.args,
                    "--tool",
                    tool.name,
                    "--args",
                    JSON.stringify(args),
                ],
                {
                    env: { ...process.env, ...provider.env },
                }
            );

            let stdout = "";
            let stderr = "";

            proc.stdout.on("data", (data: Buffer) => {
                stdout += data.toString();
            });

            proc.stderr.on("data", (data: Buffer) => {
                stderr += data.toString();
            });

            proc.on("close", (code: number | null) => {
                if (code !== 0) {
                    reject(new Error(`Tool execution failed: ${stderr}`));
                    return;
                }

                try {
                    const result = JSON.parse(stdout);
                    resolve(result);
                } catch {
                    // Return raw stdout if not JSON
                    resolve(stdout);
                }
            });

            proc.on("error", (error: Error) => {
                reject(error);
            });

            // Timeout after 30 seconds
            setTimeout(() => {
                proc.kill();
                reject(new Error("Tool execution timed out"));
            }, 30000);
        });
    }

    /**
     * Get all available tools
     */
    getAvailableTools(): McpTool[] {
        return Array.from(this.tools.values());
    }

    /**
     * Get tools by provider
     */
    getToolsByProvider(providerName: string): McpTool[] {
        return Array.from(this.tools.values()).filter(
            (tool) => tool.provider === providerName
        );
    }

    /**
     * Check if a tool exists
     */
    hasTool(toolName: string): boolean {
        return this.tools.has(toolName);
    }

    /**
     * Simple TOML parser (basic implementation)
     */
    private parseToml(content: string): Record<string, unknown> {
        // This is a simplified parser - in production, use a proper TOML library
        const result: Record<string, unknown> = {};
        const lines = content.split("\n");
        let currentSection = result;
        let currentArraySection: Record<string, unknown>[] | null = null;
        let currentArrayName = "";

        for (const line of lines) {
            const trimmed = line.trim();

            // Skip comments and empty lines
            if (!trimmed || trimmed.startsWith("#")) {
                continue;
            }

            // Array table: [[section.name]]
            if (trimmed.startsWith("[[") && trimmed.endsWith("]]")) {
                const sectionName = trimmed.slice(2, -2);
                const parts = sectionName.split(".");

                let target: Record<string, unknown> = result;
                for (let i = 0; i < parts.length - 1; i++) {
                    if (!target[parts[i]]) {
                        target[parts[i]] = {};
                    }
                    target = target[parts[i]] as Record<string, unknown>;
                }

                const lastPart = parts[parts.length - 1];
                if (!target[lastPart]) {
                    target[lastPart] = [];
                }

                currentArraySection = target[lastPart] as Record<
                    string,
                    unknown
                >[];
                currentArrayName = lastPart;
                const newObj = {};
                currentArraySection.push(newObj);
                currentSection = newObj;
                continue;
            }

            // Regular table: [section]
            if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
                currentArraySection = null;
                const sectionName = trimmed.slice(1, -1);
                const parts = sectionName.split(".");

                let target: Record<string, unknown> = result;
                for (const part of parts) {
                    if (!target[part]) {
                        target[part] = {};
                    }
                    target = target[part] as Record<string, unknown>;
                }
                currentSection = target;
                continue;
            }

            // Key-value pair
            const equalIndex = trimmed.indexOf("=");
            if (equalIndex > 0) {
                const key = trimmed.slice(0, equalIndex).trim();
                let value = trimmed.slice(equalIndex + 1).trim();

                // Parse value
                if (value.startsWith('"') && value.endsWith('"')) {
                    value = value.slice(1, -1);
                } else if (value === "true") {
                    value = true as unknown as string;
                } else if (value === "false") {
                    value = false as unknown as string;
                } else if (value.startsWith("[") && value.endsWith("]")) {
                    // Simple array parsing
                    value = value
                        .slice(1, -1)
                        .split(",")
                        .map((v) =>
                            v.trim().replace(/^["']|["']$/g, "")
                        ) as unknown as string;
                } else if (!Number.isNaN(Number(value))) {
                    value = Number(value) as unknown as string;
                }

                currentSection[key] = value;
            }
        }

        return result;
    }
}

/**
 * Create MCP tool manager with workspace context
 */
export async function createMcpToolManager(
    outputChannel: vscode.OutputChannel
): Promise<McpToolManager | null> {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders || workspaceFolders.length === 0) {
        outputChannel.appendLine("[MCP] No workspace folder found");
        return null;
    }

    const manager = new McpToolManager(outputChannel);
    await manager.loadProviders(workspaceFolders[0].uri.fsPath);

    return manager;
}
