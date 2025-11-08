import * as vscode from "vscode";
import { BaseParticipant, type ParticipantContext } from "../types/participant";

/**
 * Workspace participant provides workspace-wide context
 */
export class WorkspaceParticipant extends BaseParticipant {
    public readonly id = "workspace";
    public readonly displayName = "Workspace";
    public readonly description = "Provides workspace-wide context and file information";
    public readonly icon = "folder";

    canHandle(context: ParticipantContext): boolean {
        // Always available when workspace is open
        return context.workspace !== undefined;
    }

    async resolveReferenceContext(message: string, context: ParticipantContext): Promise<string> {
        if (!this.extractMention(message, this.id)) {
            return message;
        }

        const workspace = context.workspace;
        if (!workspace) {
            return message;
        }

        // Clean the message first
        const cleanedMessage = this.cleanMessage(message, this.id);

        // Gather workspace context
        const workspaceName = workspace.name;
        const workspacePath = workspace.uri.fsPath;
        
        // Get workspace statistics
        const files = await vscode.workspace.findFiles('**/*', '**/node_modules/**', 100);
        const fileCount = files.length;
        
        // Get open editors
        const openEditors = vscode.window.visibleTextEditors;
        const openFiles = openEditors
            .map(editor => {
                const fileName = editor.document.fileName;
                if (this.isFileInWorkspace(fileName, context)) {
                    const relativePath = vscode.workspace.asRelativePath(fileName, false);
                    return `- ${relativePath}`;
                }
                return null;
            })
            .filter(Boolean)
            .join('\n');

        // Build workspace context
        let workspaceContext = `\n\n## Workspace Context\n`;
        workspaceContext += `Workspace: ${workspaceName}\n`;
        workspaceContext += `Path: ${workspacePath}\n`;
        workspaceContext += `Files in workspace: ${fileCount}\n`;
        
        if (openFiles) {
            workspaceContext += `\nCurrently open files:\n${openFiles}\n`;
        }

        // Add recent files if available
        if (context.activeFile) {
            const relativePath = vscode.workspace.asRelativePath(context.activeFile.path, false);
            workspaceContext += `\nActive file: ${relativePath}\n`;
        }

        return `${cleanedMessage}${workspaceContext}`;
    }
}