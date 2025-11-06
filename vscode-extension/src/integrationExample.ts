/**
 * Quick Integration Example for VTCode Chat with MCP
 *
 * Copy this code to your extension.ts activate() function
 */

import * as vscode from "vscode";
import { VtcodeTerminalManager } from "./agentTerminal";
import { createMcpEnabledChat } from "./mcpChatAdapter";

export async function activate(context: vscode.ExtensionContext) {
    // Create output channel for logging
    const outputChannel = vscode.window.createOutputChannel("VTCode Chat");
    context.subscriptions.push(outputChannel);

    // Create terminal manager (assumes you already have this)
    const terminalManager = new VtcodeTerminalManager(context);
    context.subscriptions.push(terminalManager);

    // Create MCP-enabled chat provider
    try {
        const chatProvider = await createMcpEnabledChat(
            context,
            terminalManager,
            outputChannel
        );

        // Register webview provider
        context.subscriptions.push(
            vscode.window.registerWebviewViewProvider(
                "vtcodeChat", // Must match package.json viewsId
                chatProvider,
                {
                    webviewOptions: {
                        retainContextWhenHidden: true,
                    },
                }
            )
        );

        // Register commands
        context.subscriptions.push(
            vscode.commands.registerCommand("vtcode.chat.clear", () => {
                vscode.commands.executeCommand(
                    "workbench.action.webview.reloadWebviewAction"
                );
            })
        );

        context.subscriptions.push(
            vscode.commands.registerCommand("vtcode.chat.export", async () => {
                const result = await vscode.window.showSaveDialog({
                    defaultUri: vscode.Uri.file("vtcode-transcript.json"),
                    filters: {
                        JSON: ["json"],
                    },
                });

                if (result) {
                    // Export implementation would go here
                    vscode.window.showInformationMessage(
                        `Transcript exported to ${result.fsPath}`
                    );
                }
            })
        );

        outputChannel.appendLine("[Chat] Extension activated successfully");
    } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error);
        outputChannel.appendLine(`[Chat] Activation failed: ${errorMsg}`);
        vscode.window.showErrorMessage(
            `Failed to activate VTCode Chat: ${errorMsg}`
        );
    }
}

/**
 * Alternative: Composition approach for more control
 */
export async function activateWithComposition(
    context: vscode.ExtensionContext
) {
    const outputChannel = vscode.window.createOutputChannel("VTCode Chat");
    const terminalManager = new VtcodeTerminalManager(context);

    // Import factory functions
    const { createChatWithMcp } = await import("./mcpChatAdapter");

    // Create components separately
    const { chatProvider, mcpAdapter } = await createChatWithMcp(
        context,
        terminalManager,
        outputChannel
    );

    // Register provider
    context.subscriptions.push(
        vscode.window.registerWebviewViewProvider("vtcodeChat", chatProvider)
    );

    // Use MCP adapter for additional functionality
    const mcpTools = mcpAdapter.getAvailableMcpTools();
    outputChannel.appendLine(`[MCP] ${mcpTools.length} tools available`);

    // Register MCP-specific commands
    context.subscriptions.push(
        vscode.commands.registerCommand("vtcode.mcp.listTools", async () => {
            const toolsList = await mcpAdapter.listMcpTools();
            vscode.window.showInformationMessage(toolsList);
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand(
            "vtcode.mcp.reloadProviders",
            async () => {
                try {
                    const result = await mcpAdapter.reloadMcpProviders();
                    vscode.window.showInformationMessage(result);
                } catch (error) {
                    vscode.window.showErrorMessage(
                        `Failed to reload: ${
                            error instanceof Error
                                ? error.message
                                : String(error)
                        }`
                    );
                }
            }
        )
    );
}

/**
 * Package.json configuration needed:
 */
const packageJsonContributions = {
    contributes: {
        viewsContainers: {
            activitybar: [
                {
                    id: "vtcode-sidebar",
                    title: "VTCode",
                    icon: "$(comment-discussion)",
                },
            ],
        },
        views: {
            "vtcode-sidebar": [
                {
                    type: "webview",
                    id: "vtcodeChat",
                    name: "Chat",
                    contextualTitle: "VTCode Chat",
                },
            ],
        },
        commands: [
            {
                command: "vtcode.chat.clear",
                title: "VTCode: Clear Chat",
                icon: "$(clear-all)",
            },
            {
                command: "vtcode.chat.export",
                title: "VTCode: Export Chat Transcript",
                icon: "$(export)",
            },
            {
                command: "vtcode.mcp.listTools",
                title: "VTCode: List MCP Tools",
            },
            {
                command: "vtcode.mcp.reloadProviders",
                title: "VTCode: Reload MCP Providers",
            },
        ],
        menus: {
            "view/title": [
                {
                    command: "vtcode.chat.clear",
                    when: "view == vtcodeChat",
                    group: "navigation",
                },
                {
                    command: "vtcode.chat.export",
                    when: "view == vtcodeChat",
                    group: "navigation",
                },
            ],
            commandPalette: [
                {
                    command: "vtcode.mcp.listTools",
                    when: "vtcode.mcpEnabled",
                },
                {
                    command: "vtcode.mcp.reloadProviders",
                    when: "vtcode.mcpEnabled",
                },
            ],
        },
        configuration: {
            title: "VTCode Chat",
            properties: {
                "vtcode.chat.autoApproveTools": {
                    type: "boolean",
                    default: false,
                    description: "Automatically approve tool executions",
                },
                "vtcode.chat.maxHistoryLength": {
                    type: "number",
                    default: 100,
                    description: "Maximum messages in chat history",
                },
                "vtcode.chat.enableStreaming": {
                    type: "boolean",
                    default: true,
                    description: "Enable streaming responses",
                },
                "vtcode.chat.showTimestamps": {
                    type: "boolean",
                    default: true,
                    description: "Show message timestamps",
                },
                "vtcode.mcp.enabled": {
                    type: "boolean",
                    default: true,
                    description: "Enable MCP tool integration",
                },
            },
        },
    },
};

// Export for reference
export { packageJsonContributions };
