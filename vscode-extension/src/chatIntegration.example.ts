/**
 * Example integration snippet for extension.ts
 *
 * Add this code to your activate() function in extension.ts
 */

import { ChatViewProvider } from "./chatView";
import { createVtcodeBackend } from "./vtcodeBackend";

// In activate() function:
export function activateChatFeature(
    context: vscode.ExtensionContext,
    terminalManager: VtcodeTerminalManager,
    outputChannel: vscode.OutputChannel
) {
    // Create backend
    void createVtcodeBackend(outputChannel).then((backend) => {
        if (!backend) {
            outputChannel.appendLine(
                "[warning] vtcode CLI not available, chat features will be limited"
            );
        }

        // Register chat view provider
        const chatProvider = new ChatViewProvider(context, terminalManager);

        context.subscriptions.push(
            vscode.window.registerWebviewViewProvider(
                ChatViewProvider.viewType,
                chatProvider
            )
        );

        // Register commands
        context.subscriptions.push(
            vscode.commands.registerCommand("vtcode.chat.clear", () => {
                chatProvider.clearTranscript();
            })
        );

        context.subscriptions.push(
            vscode.commands.registerCommand("vtcode.chat.export", async () => {
                await chatProvider.exportTranscript();
            })
        );

        outputChannel.appendLine("[info] VTCode chat features activated");
    });
}

// Package.json updates needed:
const packageJsonUpdates = {
    contributes: {
        viewsContainers: {
            activitybar: [
                {
                    id: "vtcode-sidebar",
                    title: "VTCode",
                    icon: "media/vtcode-icon.svg",
                },
            ],
        },
        views: {
            "vtcode-sidebar": [
                {
                    type: "webview",
                    id: "vtcodeChat",
                    name: "Chat",
                    icon: "media/chat-icon.svg",
                    contextualTitle: "VTCode Chat",
                },
                {
                    id: "vtcodeQuickActionsView",
                    name: "Quick Actions",
                },
                {
                    id: "vtcodeWorkspaceStatusView",
                    name: "Workspace Status",
                },
            ],
        },
        commands: [
            {
                command: "vtcode.chat.clear",
                title: "VTCode: Clear Chat Transcript",
                icon: "$(clear-all)",
            },
            {
                command: "vtcode.chat.export",
                title: "VTCode: Export Chat Transcript",
                icon: "$(export)",
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
        },
        configuration: {
            title: "VTCode Chat",
            properties: {
                "vtcode.chat.autoApproveTools": {
                    type: "boolean",
                    default: false,
                    description:
                        "Automatically approve tool executions without confirmation",
                },
                "vtcode.chat.maxHistoryLength": {
                    type: "number",
                    default: 100,
                    description:
                        "Maximum number of messages to keep in chat history",
                },
                "vtcode.chat.enableStreaming": {
                    type: "boolean",
                    default: true,
                    description: "Enable streaming responses from the agent",
                },
                "vtcode.chat.showTimestamps": {
                    type: "boolean",
                    default: true,
                    description: "Show timestamps for each message",
                },
                "vtcode.chat.defaultModel": {
                    type: "string",
                    default: "gemini-2.5-flash-lite",
                    description: "Default LLM model to use for chat",
                },
            },
        },
    },
};
