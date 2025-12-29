import * as vscode from "vscode";
import { BaseCommand, type CommandContext } from "../types/command";
import { runVtcodeCommand } from "../utils/vtcodeRunner";

/**
 * Command to ask the VT Code agent a question
 */
export class AskCommand extends BaseCommand {
    public readonly id = "vtcode.askAgent";
    public readonly title = "Ask Agent";
    public readonly description = "Ask the VT Code agent a question";
    public readonly icon = "comment-discussion";

    async execute(context: CommandContext): Promise<void> {
        if (!this.ensureCliAvailable(context)) {
            return;
        }

        const question = await vscode.window.showInputBox({
            prompt: "What would you like the VT Code agent to help with?",
            placeHolder: "Summarize src/main.rs",
            ignoreFocusOut: true,
        });

        if (!question || !question.trim()) {
            return;
        }

        try {
            // Note: IDE context integration would be added here
            const promptWithContext = question; // Simplified for now

            await runVtcodeCommand(["ask", promptWithContext], {
                title: "Asking VT Code…",
                output: context.output,
            });
            void vscode.window.showInformationMessage(
                "VT Code finished processing your request. Check the VT Code output channel for details."
            );
        } catch (error) {
            this.handleCommandError("ask", error);
        }
    }

    private handleCommandError(context: string, error: unknown): void {
        const message = error instanceof Error ? error.message : String(error);
        void vscode.window.showErrorMessage(
            `Failed to ${context} with VT Code: ${message}`
        );
    }
}
