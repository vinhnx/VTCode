/**
 * Enhanced Chat View with MCP Integration
 *
 * Extension to chatView.ts that adds MCP tool support
 */

import * as vscode from "vscode";
import { ChatViewProvider, ToolCall } from "./chatView";
import { McpToolInvocation, McpToolManager } from "./mcpTools";

/**
 * Enhanced chat view provider with MCP support
 */
export class EnhancedChatViewProvider extends ChatViewProvider {
    private mcpManager?: McpToolManager;

    constructor(
        context: vscode.ExtensionContext,
        terminalManager: import("./agentTerminal").VtcodeTerminalManager,
        mcpManager?: McpToolManager
    ) {
        super(context, terminalManager);
        this.mcpManager = mcpManager;
    }

    /**
     * Override tool invocation to support MCP tools
     */
    protected async invokeToolImplementation(
        toolName: string,
        args: Record<string, unknown>
    ): Promise<unknown> {
        // Check if this is an MCP tool (format: provider/tool)
        if (toolName.includes("/") && this.mcpManager) {
            const [provider, tool] = toolName.split("/", 2);

            const invocation: McpToolInvocation = {
                provider,
                tool,
                arguments: args,
            };

            const result = await this.mcpManager.invokeTool(invocation);

            if (!result.success) {
                throw new Error(result.error || "MCP tool invocation failed");
            }

            return result.result;
        }

        // Fall back to base implementation for non-MCP tools
        return super.invokeToolImplementation(toolName, args);
    }

    /**
     * Get available tools including MCP tools
     */
    async getAvailableTools(): Promise<ToolCall[]> {
        const baseTools = await super.getAvailableTools();

        if (!this.mcpManager) {
            return baseTools;
        }

        // Add MCP tools
        const mcpTools = this.mcpManager.getAvailableTools();
        const mcpToolCalls: ToolCall[] = mcpTools.map((tool) => ({
            id: `mcp_${tool.provider}_${tool.name}`,
            name: `${tool.provider}/${tool.name}`,
            arguments: {},
        }));

        return [...baseTools, ...mcpToolCalls];
    }

    /**
     * Handle special MCP commands
     */
    protected async handleSystemCommand(command: string): Promise<void> {
        const cmd = command.slice(1).trim().toLowerCase();
        const args = cmd.split(/\s+/);
        const baseCmd = args[0];

        // Handle MCP-specific commands
        if (baseCmd === "mcp") {
            const subCmd = args[1];

            switch (subCmd) {
                case "list":
                    await this.listMcpTools();
                    break;

                case "providers":
                    await this.listMcpProviders();
                    break;

                case "reload":
                    await this.reloadMcpProviders();
                    break;

                default:
                    this.sendSystemMessage(
                        `Unknown MCP command: ${subCmd}. Available: list, providers, reload`
                    );
            }
            return;
        }

        // Fall back to base implementation
        await super.handleSystemCommand(command);
    }

    /**
     * List available MCP tools
     */
    private async listMcpTools(): Promise<void> {
        if (!this.mcpManager) {
            this.sendSystemMessage("MCP is not enabled", "warning");
            return;
        }

        const tools = this.mcpManager.getAvailableTools();

        if (tools.length === 0) {
            this.sendSystemMessage("No MCP tools available");
            return;
        }

        const toolsList = tools
            .map(
                (tool) => `- ${tool.provider}/${tool.name}: ${tool.description}`
            )
            .join("\n");

        this.sendSystemMessage(
            `📦 Available MCP Tools (${tools.length}):\n\n${toolsList}`
        );
    }

    /**
     * List MCP providers
     */
    private async listMcpProviders(): Promise<void> {
        if (!this.mcpManager) {
            this.sendSystemMessage("MCP is not enabled", "warning");
            return;
        }

        const tools = this.mcpManager.getAvailableTools();
        const providers = new Set(tools.map((t) => t.provider));

        if (providers.size === 0) {
            this.sendSystemMessage("No MCP providers configured");
            return;
        }

        const providersList = Array.from(providers)
            .map((provider) => {
                const providerTools =
                    this.mcpManager!.getToolsByProvider(provider);
                return `- ${provider} (${providerTools.length} tools)`;
            })
            .join("\n");

        this.sendSystemMessage(
            `🔌 Active MCP Providers (${providers.size}):\n\n${providersList}`
        );
    }

    /**
     * Reload MCP providers
     */
    private async reloadMcpProviders(): Promise<void> {
        if (!this.mcpManager) {
            this.sendSystemMessage("MCP is not enabled", "warning");
            return;
        }

        this.sendSystemMessage("🔄 Reloading MCP providers...");

        const workspaceFolders = vscode.workspace.workspaceFolders;
        if (!workspaceFolders || workspaceFolders.length === 0) {
            this.sendSystemMessage("No workspace folder found", "error");
            return;
        }

        try {
            await this.mcpManager.loadProviders(workspaceFolders[0].uri.fsPath);
            const tools = this.mcpManager.getAvailableTools();
            this.sendSystemMessage(
                `✅ Reloaded MCP providers. ${tools.length} tools available.`
            );
        } catch (error) {
            this.sendSystemMessage(
                `❌ Failed to reload MCP providers: ${
                    error instanceof Error ? error.message : String(error)
                }`,
                "error"
            );
        }
    }
}

/**
 * Factory function to create enhanced chat view with MCP support
 */
export async function createEnhancedChatView(
    context: vscode.ExtensionContext,
    terminalManager: import("./agentTerminal").VtcodeTerminalManager,
    outputChannel: vscode.OutputChannel
): Promise<EnhancedChatViewProvider> {
    // Try to create MCP manager
    const { createMcpToolManager } = await import("./mcpTools");
    const mcpManager = await createMcpToolManager(outputChannel);

    if (mcpManager) {
        const tools = mcpManager.getAvailableTools();
        outputChannel.appendLine(
            `[Chat] Initialized with ${tools.length} MCP tools`
        );
    } else {
        outputChannel.appendLine("[Chat] Initialized without MCP support");
    }

    return new EnhancedChatViewProvider(context, terminalManager, mcpManager);
}
