import * as vscode from "vscode";
import { ICommand, CommandContext } from "./types/command";

/**
 * Registry for managing VTCode commands
 */
export class CommandRegistry {
    private commands = new Map<string, ICommand>();
    private disposables: vscode.Disposable[] = [];

    /**
     * Register a command with the registry
     */
    public register(command: ICommand): void {
        this.commands.set(command.id, command);
        
        // Register with VS Code command system
        const disposable = vscode.commands.registerCommand(
            command.id,
            async () => {
                const context = this.buildCommandContext();
                if (command.canExecute(context)) {
                    try {
                        await command.execute(context);
                    } catch (error) {
                        const message = error instanceof Error ? error.message : String(error);
                        void vscode.window.showErrorMessage(
                            `Command "${command.title}" failed: ${message}`
                        );
                    }
                }
            }
        );
        
        this.disposables.push(disposable);
    }

    /**
     * Register multiple commands at once
     */
    public registerAll(commands: ICommand[]): void {
        for (const command of commands) {
            this.register(command);
        }
    }

    /**
     * Get a command by ID
     */
    public get(id: string): ICommand | undefined {
        return this.commands.get(id);
    }

    /**
     * Get all registered commands
     */
    public getAll(): ICommand[] {
        return Array.from(this.commands.values());
    }

    /**
     * Unregister a command
     */
    public unregister(id: string): void {
        this.commands.delete(id);
    }

    /**
     * Clear all registered commands
     */
    public clear(): void {
        this.commands.clear();
        this.disposables.forEach(d => d.dispose());
        this.disposables = [];
    }

    /**
     * Build command context from current VS Code state
     */
    private buildCommandContext(): CommandContext {
        return {
            workspaceFolder: vscode.workspace.workspaceFolders?.[0],
            activeTextEditor: vscode.window.activeTextEditor,
            selection: vscode.window.activeTextEditor?.selection,
            terminal: vscode.window.activeTerminal,
        };
    }

    /**
     * Dispose of all registered commands
     */
    public dispose(): void {
        this.clear();
    }
}