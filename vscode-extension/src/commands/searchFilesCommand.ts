import * as vscode from "vscode";
import { BaseCommand, type CommandContext } from "../types/command";
import { runVtcodeCommand } from "../utils/vtcodeRunner";

/**
 * Command to search files with pattern and exclusions
 *
 * Combined fuzzy search and filtering operation for advanced queries.
 * Uses the optimized file search bridge for fast results.
 */
export class SearchFilesCommand extends BaseCommand {
    public readonly id = "vtcode.searchFiles";
    public readonly title = "Search Files";
    public readonly description = "Search files with pattern and exclusions";
    public readonly icon = "search-fuzzy";

    async execute(context: CommandContext): Promise<void> {
        if (!this.ensureCliAvailable(context)) {
            return;
        }

        const pattern = await vscode.window.showInputBox({
            prompt: "Enter search pattern (e.g., 'test', 'component')",
            placeHolder: "Search pattern",
            ignoreFocusOut: true,
        });

        if (!pattern || !pattern.trim()) {
            return;
        }

        const excludeStr = await vscode.window.showInputBox({
            prompt: "Enter exclusion patterns (comma-separated)",
            placeHolder: "e.g., '**/__tests__/**,dist/**'",
            ignoreFocusOut: true,
        });

        if (!excludeStr || !excludeStr.trim()) {
            void vscode.window.showWarningMessage("Exclusion pattern is required");
            return;
        }

        try {
            const args = [
                "find-files",
                "--pattern",
                pattern.trim(),
                "--exclude",
                excludeStr.trim(),
            ];

            await runVtcodeCommand(args, {
                title: `Searching for "${pattern}"…`,
                output: context.output,
            });

            void vscode.window.showInformationMessage(
                "File search completed. Check the VT Code output channel for results."
            );
        } catch (error) {
            this.handleCommandError("search files", error);
        }
    }

    private handleCommandError(context: string, error: unknown): void {
        const message = error instanceof Error ? error.message : String(error);
        void vscode.window.showErrorMessage(
            `Failed to ${context}: ${message}`
        );
    }
}
