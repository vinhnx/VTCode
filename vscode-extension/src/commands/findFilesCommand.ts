import * as vscode from "vscode";
import { BaseCommand, type CommandContext } from "../types/command";
import { runVtcodeCommand } from "../utils/vtcodeRunner";

/**
 * Command to find files using fuzzy pattern matching
 *
 * Uses the optimized file search bridge for fast, parallel file discovery.
 * Supports fuzzy filename matching and respects .gitignore files.
 */
export class FindFilesCommand extends BaseCommand {
    public readonly id = "vtcode.findFiles";
    public readonly title = "Find Files";
    public readonly description = "Find files using fuzzy pattern matching";
    public readonly icon = "search";

    async execute(context: CommandContext): Promise<void> {
        if (!this.ensureCliAvailable(context)) {
            return;
        }

        const pattern = await vscode.window.showInputBox({
            prompt: "Enter file pattern (e.g., 'main', 'component.rs')",
            placeHolder: "Pattern to search for",
            ignoreFocusOut: true,
        });

        if (!pattern || !pattern.trim()) {
            return;
        }

        const limitStr = await vscode.window.showInputBox({
            prompt: "Maximum number of results (leave empty for default 50)",
            placeHolder: "50",
            ignoreFocusOut: true,
            validateInput: (value) => {
                if (!value) return null;
                const num = parseInt(value, 10);
                return isNaN(num) || num <= 0 ? "Please enter a positive number" : null;
            },
        });

        const limit = limitStr ? parseInt(limitStr, 10) : undefined;

        try {
            const args = ["find-files", "--pattern", pattern];
            if (limit) {
                args.push("--limit", limit.toString());
            }

            await runVtcodeCommand(args, {
                title: `Finding files matching "${pattern}"…`,
                output: context.output,
            });

            void vscode.window.showInformationMessage(
                `File search completed. Check the VT Code output channel for results.`
            );
        } catch (error) {
            this.handleCommandError("find files", error);
        }
    }

    private handleCommandError(context: string, error: unknown): void {
        const message = error instanceof Error ? error.message : String(error);
        void vscode.window.showErrorMessage(
            `Failed to ${context}: ${message}`
        );
    }
}
