#!/bin/bash
# Integration script for VTCode Chat Sidebar Extension
# Run this from the vscode-extension directory

set -e

echo "🚀 Integrating VTCode Chat Sidebar Extension..."

# Check if we're in the right directory
if [ ! -f "package.json" ]; then
    echo "❌ Error: package.json not found. Please run this script from vscode-extension directory"
    exit 1
fi

echo "📝 Updating package.json..."

# Backup package.json
cp package.json package.json.backup

# Add chat view configuration to package.json
# This would normally be done with a JSON processor like jq
echo "⚠️  Manual step required: Add the following to package.json contributes section:"
cat << 'EOF'

{
  "contributes": {
    "viewsContainers": {
      "activitybar": [
        {
          "id": "vtcode-sidebar",
          "title": "VTCode",
          "icon": "media/vtcode-icon.svg"
        }
      ]
    },
    "views": {
      "vtcode-sidebar": [
        {
          "type": "webview",
          "id": "vtcodeChat",
          "name": "Chat",
          "icon": "$(comment-discussion)",
          "contextualTitle": "VTCode Chat"
        },
        {
          "id": "vtcodeQuickActionsView",
          "name": "Quick Actions"
        },
        {
          "id": "vtcodeWorkspaceStatusView",
          "name": "Workspace Status"
        }
      ]
    },
    "commands": [
      {
        "command": "vtcode.chat.clear",
        "title": "VTCode: Clear Chat Transcript",
        "icon": "$(clear-all)"
      },
      {
        "command": "vtcode.chat.export",
        "title": "VTCode: Export Chat Transcript",
        "icon": "$(export)"
      }
    ],
    "menus": {
      "view/title": [
        {
          "command": "vtcode.chat.clear",
          "when": "view == vtcodeChat",
          "group": "navigation"
        },
        {
          "command": "vtcode.chat.export",
          "when": "view == vtcodeChat",
          "group": "navigation"
        }
      ]
    },
    "configuration": {
      "title": "VTCode Chat",
      "properties": {
        "vtcode.chat.autoApproveTools": {
          "type": "boolean",
          "default": false,
          "description": "Automatically approve tool executions without confirmation"
        },
        "vtcode.chat.maxHistoryLength": {
          "type": "number",
          "default": 100,
          "description": "Maximum number of messages to keep in chat history"
        },
        "vtcode.chat.enableStreaming": {
          "type": "boolean",
          "default": true,
          "description": "Enable streaming responses from the agent"
        },
        "vtcode.chat.showTimestamps": {
          "type": "boolean",
          "default": true,
          "description": "Show timestamps for each message"
        },
        "vtcode.chat.defaultModel": {
          "type": "string",
          "default": "gemini-2.5-flash-lite",
          "description": "Default LLM model to use for chat"
        }
      }
    }
  }
}

EOF

echo ""
echo "📝 Updating extension.ts..."
echo "⚠️  Manual step required: Add the following to your activate() function in src/extension.ts:"
cat << 'EOF'

// Import chat components
import { ChatViewProvider } from "./chatView";
import { createVtcodeBackend } from "./vtcodeBackend";

// In activate() function, after terminalManager is created:
if (terminalManager) {
    // Initialize chat backend
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

        // Register chat commands
        context.subscriptions.push(
            vscode.commands.registerCommand("vtcode.chat.clear", () => {
                // Clear will be handled by webview message
                vscode.window.showInformationMessage("Chat cleared");
            })
        );

        context.subscriptions.push(
            vscode.commands.registerCommand("vtcode.chat.export", async () => {
                // Export will be triggered via webview
                vscode.window.showInformationMessage("Exporting chat transcript...");
            })
        );

        outputChannel.appendLine("[info] VTCode chat features activated");
    });
}

EOF

echo ""
echo "✅ Integration steps provided!"
echo ""
echo "📋 Next steps:"
echo "1. Review and merge package.json changes"
echo "2. Add the code snippet to src/extension.ts"
echo "3. Run: npm run compile"
echo "4. Test: Press F5 to launch Extension Development Host"
echo "5. Look for the VTCode icon in the activity bar"
echo ""
echo "📚 Documentation:"
echo "- CHAT_EXTENSION.md - Architecture and features"
echo "- CHAT_QUICKSTART.md - User guide"
echo "- docs/CHAT_SIDEBAR_IMPLEMENTATION.md - Implementation summary"
echo ""
echo "🎉 Done! Check the files created:"
echo "  - src/chatView.ts"
echo "  - src/vtcodeBackend.ts"
echo "  - media/chat-view.css"
echo "  - media/chat-view.js"
echo "  - CHAT_EXTENSION.md"
echo "  - CHAT_QUICKSTART.md"
