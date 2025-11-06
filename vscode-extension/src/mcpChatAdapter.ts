/**
 * Enhanced Chat View with MCP Integration
 *
 * Provides a composition-based approach to extend ChatViewProvider with MCP tool support.
 * Uses dependency injection and delegation patterns for better maintainability.
 */

import * as vscode from "vscode";
import { VtcodeTerminalManager } from "./agentTerminal";
import { ChatViewProvider, ToolCall } from "./chatView";
import { McpTool, McpToolInvocation, McpToolManager } from "./mcpTools";

/**
 * Adapter class that wraps ChatViewProvider and adds MCP functionality
 * through composition instead of inheritance
 */
export class McpChatAdapter {
    private chatProvider: ChatViewProvider;
    private mcpManager: McpToolManager | null;
    private outputChannel: vscode.OutputChannel;

    constructor(
        chatProvider: ChatViewProvider,
        mcpManager: McpToolManager | null,
        outputChannel: vscode.OutputChannel
    ) {
        this.chatProvider = chatProvider;
        this.mcpManager = mcpManager;
        this.outputChannel = outputChannel;
    }

    /**
     * Check if a tool name is an MCP tool (format: provider/tool)
     */
    isMcpTool(toolName: string): boolean {
        return toolName.includes("/") && this.mcpManager !== null;
    }

    /**
     * Invoke an MCP tool
     */
    async invokeMcpTool(
        toolName: string,
        args: Record<string, unknown>
    ): Promise<unknown> {
        if (!this.mcpManager) {
            throw new Error("MCP manager not initialized");
        }

        const [provider, tool] = toolName.split("/", 2);
        if (!provider || !tool) {
            throw new Error(
                `Invalid MCP tool format: ${toolName}. Expected: provider/tool`
            );
        }

        const invocation: McpToolInvocation = {
            provider,
            tool,
            arguments: args,
        };

        this.outputChannel.appendLine(`[MCP] Invoking tool: ${toolName}`);
        this.outputChannel.appendLine(
            `[MCP] Arguments: ${JSON.stringify(args, null, 2)}`
        );

        const startTime = Date.now();
        const result = await this.mcpManager.invokeTool(invocation);
        const duration = Date.now() - startTime;

        if (!result.success) {
            this.outputChannel.appendLine(
                `[MCP] Tool failed after ${duration}ms: ${result.error}`
            );
            throw new Error(result.error || "MCP tool invocation failed");
        }

        this.outputChannel.appendLine(`[MCP] Tool succeeded in ${duration}ms`);
        return result.result;
    }

    /**
     * Get all available MCP tools
     */
    getAvailableMcpTools(): McpTool[] {
        if (!this.mcpManager) {
            return [];
        }
        return this.mcpManager.getAvailableTools();
    }

    /**
     * Get MCP tools formatted as ToolCall objects
     */
    getMcpToolCalls(): ToolCall[] {
        const mcpTools = this.getAvailableMcpTools();
        return mcpTools.map((tool) => ({
            id: `mcp_${tool.provider}_${tool.name}`,
            name: `${tool.provider}/${tool.name}`,
            arguments: {},
        }));
    }

    /**
     * List available MCP tools and send as system message
     */
    async listMcpTools(): Promise<string> {
        if (!this.mcpManager) {
            return "MCP is not enabled";
        }

        const tools = this.getAvailableMcpTools();

        if (tools.length === 0) {
            return "No MCP tools available. Check your vtcode.toml configuration.";
        }

        const toolsList = tools
            .map(
                (tool) => `- ${tool.provider}/${tool.name}: ${tool.description}`
            )
            .join("\n");

        return `📦 Available MCP Tools (${tools.length}):\n\n${toolsList}`;
    }

    /**
     * List MCP providers
     */
    async listMcpProviders(): Promise<string> {
        if (!this.mcpManager) {
            return "MCP is not enabled";
        }

        const tools = this.getAvailableMcpTools();
        const providers = new Set(tools.map((t) => t.provider));

        if (providers.size === 0) {
            return "No MCP providers configured. Add providers to your vtcode.toml file.";
        }

        const providersList = Array.from(providers)
            .map((provider) => {
                const providerTools =
                    this.mcpManager!.getToolsByProvider(provider);
                return `- ${provider} (${providerTools.length} tools)`;
            })
            .join("\n");

        return `🔌 Active MCP Providers (${providers.size}):\n\n${providersList}`;
    }

    /**
     * Reload MCP providers
     */
    async reloadMcpProviders(): Promise<string> {
        if (!this.mcpManager) {
            return "MCP is not enabled";
        }

        const workspaceFolders = vscode.workspace.workspaceFolders;
        if (!workspaceFolders || workspaceFolders.length === 0) {
            throw new Error("No workspace folder found");
        }

        try {
            this.outputChannel.appendLine("[MCP] Reloading providers...");
            await this.mcpManager.loadProviders(workspaceFolders[0].uri.fsPath);
            const tools = this.getAvailableMcpTools();
            this.outputChannel.appendLine(
                `[MCP] Reloaded successfully. ${tools.length} tools available.`
            );
            return `✅ Reloaded MCP providers. ${tools.length} tools available.`;
        } catch (error) {
            const errorMsg =
                error instanceof Error ? error.message : String(error);
            this.outputChannel.appendLine(`[MCP] Reload failed: ${errorMsg}`);
            throw new Error(`Failed to reload MCP providers: ${errorMsg}`);
        }
    }
}

/**
 * Factory function to create chat provider with MCP support
 */
export async function createChatWithMcp(
    context: vscode.ExtensionContext,
    terminalManager: VtcodeTerminalManager,
    outputChannel: vscode.OutputChannel
): Promise<{
    chatProvider: ChatViewProvider;
    mcpAdapter: McpChatAdapter;
}> {
    // Create base chat provider
    const chatProvider = new ChatViewProvider(context, terminalManager);

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

    // Create MCP adapter
    const mcpAdapter = new McpChatAdapter(
        chatProvider,
        mcpManager,
        outputChannel
    );

    return { chatProvider, mcpAdapter };
}

/**
 * Extended Chat View Provider with MCP support built-in
 * This extends the base provider and integrates MCP adapter
 */
export class McpEnabledChatProvider extends ChatViewProvider {
    protected mcpAdapter: McpChatAdapter;

    constructor(
        context: vscode.ExtensionContext,
        terminalManager: VtcodeTerminalManager,
        mcpAdapter: McpChatAdapter
    ) {
        super(context, terminalManager);
        this.mcpAdapter = mcpAdapter;
    }

    /**
     * Override tool invocation to support MCP tools
     */
    protected async invokeToolImplementation(
        toolName: string,
        args: Record<string, unknown>
    ): Promise<unknown> {
        // Check if this is an MCP tool
        if (this.mcpAdapter.isMcpTool(toolName)) {
            return await this.mcpAdapter.invokeMcpTool(toolName, args);
        }

        // Fall back to base implementation for non-MCP tools
        return super.invokeToolImplementation(toolName, args);
    }

    /**
     * Override system command handler to add MCP commands
     */
    protected async handleSystemCommand(command: string): Promise<void> {
        const cmd = command.slice(1).trim().toLowerCase();
        const args = cmd.split(/\s+/);
        const baseCmd = args[0];

        // Handle MCP-specific commands
        if (baseCmd === "mcp") {
            await this.handleMcpCommand(args.slice(1));
            return;
        }

        // Fall back to base implementation
        await super.handleSystemCommand(command);
    }

    /**
     * Handle MCP-specific commands
     */
    private async handleMcpCommand(args: string[]): Promise<void> {
        const subCmd = args[0];

        try {
            let message: string;

            switch (subCmd) {
                case "list":
                    message = await this.mcpAdapter.listMcpTools();
                    break;

                case "providers":
                    message = await this.mcpAdapter.listMcpProviders();
                    break;

                case "reload":
                    message = await this.mcpAdapter.reloadMcpProviders();
                    break;

                default:
                    message = `Unknown MCP command: ${subCmd}\n\nAvailable commands:\n- /mcp list - List available MCP tools\n- /mcp providers - Show active providers\n- /mcp reload - Reload MCP configuration`;
            }

            this.sendSystemMessage(message);
        } catch (error) {
            this.sendSystemMessage(
                `MCP command failed: ${
                    error instanceof Error ? error.message : String(error)
                }`,
                "error"
            );
        }
    }
}

/**
 * Comprehensive factory function that creates MCP-enabled chat provider
 */
export async function createMcpEnabledChat(
    context: vscode.ExtensionContext,
    terminalManager: VtcodeTerminalManager,
    outputChannel: vscode.OutputChannel
): Promise<McpEnabledChatProvider> {
    // Create MCP infrastructure
    const { createMcpToolManager } = await import("./mcpTools");
    const mcpManager = await createMcpToolManager(outputChannel);

    if (mcpManager) {
        const tools = mcpManager.getAvailableTools();
        outputChannel.appendLine(
            `[MCP] Loaded ${tools.length} tools from ${
                new Set(tools.map((t) => t.provider)).size
            } providers`
        );
    } else {
        outputChannel.appendLine(
            "[MCP] Not available - chat will use basic tools only"
        );
    }

    // Create adapter
    const mcpAdapter = new McpChatAdapter(
        null as any,
        mcpManager,
        outputChannel
    );

    // Create and return MCP-enabled provider
    return new McpEnabledChatProvider(context, terminalManager, mcpAdapter);
}
