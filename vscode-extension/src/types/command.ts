import * as vscode from "vscode";

/**
 * Context provided to commands during execution
 */
export interface CommandContext {
    /** Current workspace folder, if any */
    workspaceFolder?: vscode.WorkspaceFolder;
    /** Active text editor, if any */
    activeTextEditor?: vscode.TextEditor;
    /** Current text selection, if any */
    selection?: vscode.Selection;
    /** Active terminal, if any */
    terminal?: vscode.Terminal;
    /** VT Code backend instance */
    backend?: any;
    /** Output channel for logging */
    output: vscode.OutputChannel;
}

/**
 * Base interface for all VT Code commands
 */
export interface ICommand {
    /** Unique command identifier */
    readonly id: string;
    /** Human-readable command title */
    readonly title: string;
    /** Optional command description */
    readonly description?: string;
    /** Optional icon for UI display */
    readonly icon?: string;

    /**
     * Execute the command with the given context
     * @param context Command execution context
     * @returns Promise that resolves when command completes
     */
    execute(context: CommandContext): Promise<void>;

    /**
     * Check if this command can be executed in the current context
     * @param context Command execution context
     * @returns true if command can execute, false otherwise
     */
    canExecute(context: CommandContext): boolean;
}

/**
 * Base class for VT Code commands providing common functionality
 */
export abstract class BaseCommand implements ICommand {
    public abstract readonly id: string;
    public abstract readonly title: string;
    public readonly description?: string;
    public readonly icon?: string;

    protected getWorkspaceRoot(context: CommandContext): string | undefined {
        if (context.activeTextEditor) {
            const folder = vscode.workspace.getWorkspaceFolder(
                context.activeTextEditor.document.uri
            );
            return folder?.uri.fsPath;
        }

        const [firstWorkspace] = vscode.workspace.workspaceFolders ?? [];
        return firstWorkspace?.uri.fsPath;
    }

    protected ensureWorkspaceTrusted(context: CommandContext): boolean {
        if (!vscode.workspace.isTrusted) {
            void vscode.window.showWarningMessage(
                "VT Code requires a trusted workspace to execute this command."
            );
            return false;
        }
        return true;
    }

    protected ensureCliAvailable(context: CommandContext): boolean {
        // This will be implemented with proper CLI detection
        return true;
    }

    abstract execute(context: CommandContext): Promise<void>;

    canExecute(context: CommandContext): boolean {
        return this.ensureWorkspaceTrusted(context);
    }
}
